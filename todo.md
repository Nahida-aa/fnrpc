先更新 `.agents/route_bytes-allocation.md`，把刚才实测修正后的结论写进去。先读一下现有内容。

这份文档有些内容已经过时（比如"96B/2blks"是 double box 时期的旧值，"7 blks"也是旧的）。我用刚刚实测的、修正后的结论来更新它，重点是：

1. 修正 `route_bytes` 现状是 **48B/1blk**（不是 2）。
2. 修正 `route_raw` vs `route_bytes` 的差距来源：之前说成"`Extensions::new()` + vtable Box::pin"，实测证明 `Extensions::new()` 空 map **不分配**，真正的 +1 blk 是 Erased 槽的 **vtable `Box::pin` 边界**。
3. 加入新的 `dhat_callpath_dispatch` 测试方法。

重写相关段落。

现在更新对比表和 route_raw 段落，修正"`Extensions::new()` 占 1 block"的错误说法。

第 1 项完成。`.agents/route_bytes-allocation.md` 已更新：

- 修正 `route_bytes` 现状是 **48B/1blk**（不再写"2 blocks / double box"）。
- 修正了对比表（`route_raw` 96B/2blks、`route_bytes` 48B/1blk）。
- **关键修正**：明确写出 `Extensions::new()` 空 map **不分配 heap**，`route_raw` 比 `route_bytes` 多的 +48B/+1blk 来自 **Erased 槽的 vtable `Box::pin` 边界**（dhat backtrace 链 2 实锤），并附上 `dhat_callpath_dispatch` 的完整证据。
- 加了"Open question"段落，把"仅在有中间件时构造 `Extensions`"和"route_raw 无中间件时走 Raw 槽"列为**待讨论、未实现**。

---

第 2 项按你说的"还需要讨论"，**我没有改代码**。在动手前，有几个点值得先对齐：

**关于"仅在有中间件时构造 `Extensions::new()`"**
- 实测已证明：空 `Extensions` **不占 heap**，所以这改动**没有任何分配收益**，纯粹是少构造一个栈上值（微小）。
- 但语义上它有意义：现在每个 Erased dispatch 都无条件构造一个永远为空的 bag，是个"为了 API 形状而存在"的浪费。问题是——`dispatch` 怎么知道"有没有中间件"？目前 `RouterSlot::Erased` 本身就意味着"可能走中间件链"，而 `ErasedHandler::call` 的签名要求 `&mut Extensions`。要让它按需构造，得把 `Extensions` 的构造推到"确实有 middleware 的那一层"，或者改成 `Option<&mut Extensions>` / 懒初始化。

**关于真正的杠杆——Erased 槽 vtable `Box::pin`**
- 那才是 route_raw 多出来的 +1 block 的真实来源。要消它，得让 `route_raw` 在无中间件时也注册进 Raw 槽（像 `route_bytes`），但这涉及 `route_raw` 的 handler 签名区别（Erased 带 `path` + `&mut Extensions`，Raw 不带）——改动比"条件构造 Extensions"大得多。

我的建议：既然空 `Extensions` 无分配收益，**先做 (b)（route_raw 走 Raw 槽）才有实际意义**；(a) 只是代码洁癖，可顺带做但不解决核心差距。

你倾向哪个方向？还是先都记下、暂不实现？
