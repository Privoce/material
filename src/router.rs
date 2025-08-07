use salvo::{Router, cors::Cors, http::Method};

use crate::api::pdf::{workhook, workhook_check};

// use crate::api::pdf::{ai_analysis, from_path, split};

pub fn build() -> Router {
    let cors = Cors::new()
        .allow_origin("*") // 允许所有来源
        .allow_methods(vec![
            Method::GET,
            Method::POST,
            Method::DELETE,
            Method::PUT,
            Method::OPTIONS,
        ]) // 允许的方法
        .allow_headers("*") // 允许所有请求头
        .expose_headers("content-disposition") // 暴露特定响应头
        .max_age(3600) // 预检请求的缓存时间
        .into_handler();

    // Router::with_path("api")
    //     .hoop(cors)
    //     .push(Router::with_path("pdf").post(from_path).get(split))
    //     .push(Router::with_path("ai").get(ai_analysis))
    Router::with_path("material").hoop(cors).push(
        Router::with_path("webhook")
            .get(workhook_check)
            .post(workhook),
    )
}
