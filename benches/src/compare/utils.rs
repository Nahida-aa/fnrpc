use xitca_http::body::RequestBody;
use xitca_http::http::{Method, Request, RequestExt, Uri};

pub fn build_get(uri: &str) -> Request<RequestExt<RequestBody>> {
    let req_ext: RequestExt<RequestBody> = RequestExt::default();
    Request::builder()
        .method(Method::GET)
        .uri(uri)
        .body(req_ext)
        .unwrap()
}

pub fn prebuild_get(uri: &str, n: usize) -> Vec<Request<RequestExt<RequestBody>>> {
    (0..n).map(|_| build_get(uri)).collect()
}
