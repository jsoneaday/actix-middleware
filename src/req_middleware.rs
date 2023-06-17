use actix_http::{h1, HttpMessage, body::{MessageBody, BoxBody}};
use actix_web::{
    Error,
    web::{Bytes, Json}, 
    dev::{Transform, ServiceRequest, Service, ServiceResponse, forward_ready, Payload}, 
    http::header::ContentType, Responder, HttpResponse
};
use futures_util::future::LocalBoxFuture;
use serde::{Deserialize, Serialize};
use std::{future::{ ready, Ready }, rc::Rc, convert::Infallible, pin::Pin, task::{Context, Poll}};
use derive_more::{Display, Error};

pub async fn do_it(data: Json<RequestMessage>) -> impl Responder {
    Json(ResponseMessage {
        msg: data.msg.clone()
    })
}

pub struct ReqAppenderMiddlewareBuilder;

impl<S, B> Transform<S, ServiceRequest> for ReqAppenderMiddlewareBuilder 
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static + MessageBody
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type InitError = ();
    type Transform = ReqAppenderMiddlewareExecutor<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(ReqAppenderMiddlewareExecutor { next_service: Rc::new(service) }))
    }
}

pub struct ReqAppenderMiddlewareExecutor<S> {
    next_service: Rc<S>
}

impl<S, B> Service<ServiceRequest> for ReqAppenderMiddlewareExecutor<S> 
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: MessageBody + 'static
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    forward_ready!(next_service);

    fn call(&self, mut req: ServiceRequest) -> Self::Future {  
        let fut = self.next_service.clone();

        Box::pin(async move {
            let request = req.request().clone();
            let body_original = req.extract::<Bytes>().await.unwrap();

            if req.content_type() == ContentType::json().0 {                               
                let body_str = String::from_utf8(body_original.to_vec());
                match body_str {
                    Ok(str) => {
                        let req_body_json: Result<RequestMessage, serde_json::Error> = serde_json::from_str(&str);
                        match req_body_json {
                            Ok(mut rbj) => {
                                rbj.msg = format!("{}. I modified the request.", rbj.msg);
                                let new_rbj_result = serde_json::to_string(&rbj);
                                let new_rbj_str = new_rbj_result.unwrap();
                                let body_final = Bytes::from(new_rbj_str);
                                req.set_payload(bytes_to_payload(body_final));
                            },
                            Err(_) => {
                                println!("Not of type RequestMessage, continuing");
                            }
                        };
                    },
                    Err(_) => {
                        println!("Payload not string, continuing");
                    }
                };
            }            
            
            let res = fut.call(req).await?;
            if res.headers().contains_key("content-type") {
                let status = res.status();
                let body = res.into_body();
                let request = res.request().clone();
                let body_bytes_result = body.try_into_bytes();             
                match body_bytes_result {
                    Ok(body_bytes) => {
                        let body_str = String::from_utf8(body_bytes.to_vec());
                        match body_str {
                            Ok(str) => {
                                let body_obj_result: Result<ResponseMessage, serde_json::Error> = serde_json::from_str(&str);
                                match body_obj_result {
                                    Ok(mut body_obj) => {
                                        body_obj.msg = format!("{}. I modified the response.", body_obj.msg);
                                        
                                        let resp = HttpResponse::build(status)
                                            .content_type("application/json")
                                            .body(body_obj);
                                        let new_res = ServiceResponse::<B>::new(request, resp);
                                        Ok(new_res)
                                    },
                                    Err(_) => {
                                        println!("Not of type ResponseMessage, continuing");
                                        Err(MiddleWareError.into()) 
                                    }
                                }
                            },
                            Err(_) => {
                                println!("To string failed");
                                Err(MiddleWareError.into()) 
                            }
                        }                          
                    },
                    Err(_) => {
                        println!("Payload not string, continuing");
                        Err(MiddleWareError.into()) 
                    }
                }
            } else {
                Ok(req.into_response(*res.response()))
            }
        })
    }
}

#[derive(Deserialize, Serialize, Clone)]
pub struct RequestMessage {
    pub msg: String
}

#[derive(Deserialize, Serialize, Clone)]
pub struct ResponseMessage {
    pub msg: String
}
impl MessageBody for ResponseMessage {
    type Error = Infallible;

    fn size(&self) -> actix_http::body::BodySize {
        actix_http::body::BodySize::Sized(self.msg.len() as u64)
    }

    fn poll_next(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Bytes, Self::Error>>> {
        std::task::Poll::Ready(Some(Ok(actix_web::web::Bytes::from(self.msg.clone()))))
    }   
}

#[derive(Debug, Error, Display)]
struct MiddleWareError;
impl actix_web::ResponseError for MiddleWareError{}

fn bytes_to_payload(buf: Bytes) -> Payload {
    let (_, mut pl) = h1::Payload::create(true);
    pl.unread_data(buf);
    Payload::from(pl)
}
