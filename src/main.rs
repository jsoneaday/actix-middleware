use actix_http::{
    body::{EitherBody, MessageBody},
    h1, HttpMessage,
};
use actix_web::{
    dev::{forward_ready, Payload, Service, ServiceRequest, ServiceResponse, Transform},
    http::header::ContentType,
    web::{Bytes, Json},
    Error, HttpResponse, Responder,
};
use actix_web::{web::post, App, HttpServer};
use derive_more::{Display, Error};
use futures_util::future::LocalBoxFuture;
use serde::{Deserialize, Serialize};
use std::{
    future::{ready, Ready},
    rc::Rc,
};

pub async fn do_it(data: Json<RequestMessage>) -> impl Responder {
    Json(ResponseMessage {
        msg: data.msg.clone(),
    })
}

pub struct ReqAppenderMiddlewareBuilder;

impl<S, B> Transform<S, ServiceRequest> for ReqAppenderMiddlewareBuilder
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static + MessageBody,
{
    type Response = ServiceResponse<EitherBody<B>>;
    type Error = Error;
    type InitError = ();
    type Transform = ReqAppenderMiddlewareExecutor<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(ReqAppenderMiddlewareExecutor {
            next_service: Rc::new(service),
        }))
    }
}

pub struct ReqAppenderMiddlewareExecutor<S> {
    next_service: Rc<S>,
}

impl<S, B> Service<ServiceRequest> for ReqAppenderMiddlewareExecutor<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<EitherBody<B>>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    forward_ready!(next_service);

    fn call(&self, mut req: ServiceRequest) -> Self::Future {
        let svc = self.next_service.clone();

        Box::pin(async move {
            let body_original = req.extract::<Bytes>().await.unwrap();

            if req.content_type() == ContentType::json().0 {
                let body_str = String::from_utf8(body_original.to_vec());
                match body_str {
                    Ok(str) => {
                        let req_body_json: Result<RequestMessage, serde_json::Error> =
                            serde_json::from_str(&str);
                        match req_body_json {
                            Ok(mut rbj) => {
                                rbj.msg = format!("{}. I modified the request.", rbj.msg);
                                let new_rbj_result = serde_json::to_string(&rbj);
                                let new_rbj_str = new_rbj_result.unwrap();
                                let body_final = Bytes::from(new_rbj_str);
                                req.set_payload(bytes_to_payload(body_final));
                            }
                            Err(_) => {
                                println!("Not of type RequestMessage, continuing");
                            }
                        };
                    }
                    Err(_) => {
                        println!("Payload not string, continuing");
                    }
                };
            }

            let res = svc.call(req).await?;

            if res.headers().contains_key("content-type") {
                let status = res.status();
                let request = res.request().clone();
                let body = res.into_body();
                let body_bytes_result = body.try_into_bytes();

                if let Ok(body_bytes) = body_bytes_result {
                    let body_str = String::from_utf8(body_bytes.to_vec());
                    if let Ok(s) = body_str {
                        let body_obj_result: Result<ResponseMessage, serde_json::Error> =
                            serde_json::from_str(&s);

                        if let Ok(mut body_obj) = body_obj_result {
                            body_obj.msg = format!("{}. I modified the response.", body_obj.msg);

                            let resp = HttpResponse::build(status).json(body_obj);
                            let new_res = ServiceResponse::new(request, resp).map_into_right_body();
                            Ok(new_res)
                        } else {
                            println!("Not of type ResponseMessage, continuing");
                            Err(MiddleWareError.into())
                        }
                    } else {
                        println!("To string failed");
                        Err(MiddleWareError.into())
                    }
                } else {
                    println!("Payload not string, continuing");
                    Err(MiddleWareError.into())
                }
            } else {
                Ok(res.map_into_left_body())
            }
        })
    }
}

#[derive(Deserialize, Serialize, Clone)]
pub struct RequestMessage {
    pub msg: String,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct ResponseMessage {
    pub msg: String,
}

#[derive(Debug, Error, Display)]
struct MiddleWareError;

impl actix_web::ResponseError for MiddleWareError {}

fn bytes_to_payload(buf: Bytes) -> Payload {
    let (_, mut pl) = h1::Payload::create(true);
    pl.unread_data(buf);
    Payload::from(pl)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    HttpServer::new(move || {
        App::new()
            .wrap(ReqAppenderMiddlewareBuilder)
            .route("/", post().to(do_it))
    })
    .bind(("127.0.0.1", 8001))?
    .run()
    .await
}
