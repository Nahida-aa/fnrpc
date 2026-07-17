//! Parse dhat-heap.json and print per-backtrace allocation summaries.
//!
//! Usage: cargo run -p benches --bin dhat_analyze -- <dhat-heap.json>
//!
//! Reads dhat-heap.json from stdin or first arg, groups allocations by backtrace,
//! and prints total bytes/blocks per unique stack.

use serde_json::Value;
use std::collections::HashMap;
use std::fs;

fn main() {
    let path = std::env::args().nth(1).unwrap_or_default();
    let raw = if path.is_empty() {
        // Read from stdin
        let mut s = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut s).unwrap();
        s
    } else {
        fs::read_to_string(&path).unwrap_or_else(|e| {
            eprintln!("Error reading {path}: {e}");
            std::process::exit(1);
        })
    };

    let v: Value = serde_json::from_str(&raw).unwrap();

    let bklt = v["bklt"].as_bool().unwrap_or(true);
    if !bklt {
        eprintln!("Warning: backtrace collection disabled (bklt=false)");
    }

    let mode = v["mode"].as_str().unwrap_or("unknown");
    let cmd = v["cmd"].as_str().unwrap_or("?");
    println!("=== dhat-heap analysis ===");
    println!("Mode: {mode}  Command: {cmd}");
    println!();

    // Parse the frame table (ftbl): array of strings, index 0 is "[root]"
    let ftbl = v["ftbl"].as_array().unwrap_or_else(|| {
        eprintln!("No ftbl found in dhat JSON");
        std::process::exit(1);
    });

    // Parse pps (program point stats)
    let pps = v["pps"].as_array().unwrap();

    let mut total_bytes: u64 = 0;
    let mut total_blocks: u64 = 0;

    // Group by leaf frame (first non-root frame in each backtrace)
    struct BtStats {
        bytes: u64,
        blocks: u64,
        backtrace: String,
    }
    let mut by_leaf: HashMap<String, BtStats> = HashMap::new();
    let mut all_entries: Vec<(u64, u64, String)> = Vec::new();

    for pp in pps {
        let bytes = pp["tb"].as_u64().unwrap_or(0);
        let blocks = pp["tbk"].as_u64().unwrap_or(0);
        total_bytes += bytes;
        total_blocks += blocks;

        // Get frame indices (fs = frame sequence)
        let fs = pp["fs"].as_array().unwrap();

        // Build backtrace string from frame indices
        // Skip frame 0 ([root]) for cleaner output
        let bt_parts: Vec<String> = fs
            .iter()
            .filter_map(|fid| {
                let idx = fid.as_u64().unwrap_or(0) as usize;
                if idx > 0 && idx < ftbl.len() {
                    let s = ftbl[idx].as_str().unwrap_or("");
                    // Extract just the function name (before the hex address and file info)
                    let clean = clean_frame(s);
                    Some(clean)
                } else {
                    None
                }
            })
            .collect();

        let bt_str = bt_parts.join("\n  ← ");
        let leaf = bt_parts.first().cloned().unwrap_or_default();

        let e = by_leaf.entry(leaf.clone()).or_insert(BtStats {
            bytes: 0,
            blocks: 0,
            backtrace: bt_str.clone(),
        });
        e.bytes += bytes;
        e.blocks += blocks;
        // Keep the most detailed backtrace
        if bt_str.len() > e.backtrace.len() {
            e.backtrace = bt_str.clone();
        }

        all_entries.push((bytes, blocks, bt_str));
    }

    println!("Total: {total_bytes}B in {total_blocks} blocks");
    println!("Per-op (n=??): ~{}B, ~{} blks",
        total_bytes as f64 / (total_blocks as f64 / 3.0).max(1.0) * 3.0,
        total_blocks as f64 / 3.0);
    println!();

    // Sort by bytes descending
    let mut sorted: Vec<_> = by_leaf.into_iter().collect();
    sorted.sort_by(|a, b| b.1.bytes.cmp(&a.1.bytes));

    println!("=== Per leaf-function allocations ===");
    println!("{:>12} {:>8}  {}", "Bytes", "Blocks", "Leaf function");
    println!("{}", "-".repeat(100));
    for (leaf, stats) in &sorted {
        println!(
            "{:>12} {:>8}  {}",
            stats.bytes, stats.blocks, leaf
        );
    }

    println!();
    println!("=== Full backtrace detail ===");
    all_entries.sort_by(|a, b| b.0.cmp(&a.0));
    for (bytes, blocks, bt) in &all_entries {
        println!("--- {bytes}B, {blocks} blks ---");
        println!("  {bt}");
        println!();
    }
}

/// Clean up a frame string: extract function name from the dhat format
/// "0xADDR: fn_name (file:line)"
fn clean_frame(s: &str) -> String {
    // Skip allocator internal frames for cleaner output
    if s.contains("<dhat::Alloc") || s.contains("alloc::alloc") {
        return format!("[alloc] {}", s);
    }
    // Try to extract just the function name
    if let Some(colon_pos) = s.find(':') {
        let after_addr = &s[colon_pos + 1..].trim();
        if let Some(paren_pos) = after_addr.find(" (") {
            return after_addr[..paren_pos].to_string();
        }
        return after_addr.to_string();
    }
    s.to_string()
}
