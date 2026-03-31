use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

pub struct S3Error {
    pub code: &'static str,
    pub message: String,
    pub http_status: StatusCode,
    pub resource: String,
}

impl S3Error {
    pub fn no_such_key(key: impl Into<String>) -> Self {
        let key = key.into();
        Self {
            code: "NoSuchKey",
            message: format!("The specified key does not exist."),
            http_status: StatusCode::NOT_FOUND,
            resource: key,
        }
    }

    pub fn no_such_bucket(bucket: impl Into<String>) -> Self {
        let bucket = bucket.into();
        Self {
            code: "NoSuchBucket",
            message: format!("The specified bucket does not exist."),
            http_status: StatusCode::NOT_FOUND,
            resource: bucket,
        }
    }

    pub fn bucket_already_exists(bucket: impl Into<String>) -> Self {
        let bucket = bucket.into();
        Self {
            code: "BucketAlreadyExists",
            message: format!("The requested bucket name is not available."),
            http_status: StatusCode::CONFLICT,
            resource: bucket,
        }
    }

    pub fn access_denied(message: impl Into<String>) -> Self {
        Self {
            code: "AccessDenied",
            message: message.into(),
            http_status: StatusCode::FORBIDDEN,
            resource: String::new(),
        }
    }

    pub fn invalid_argument(message: impl Into<String>) -> Self {
        Self {
            code: "InvalidArgument",
            message: message.into(),
            http_status: StatusCode::BAD_REQUEST,
            resource: String::new(),
        }
    }

    pub fn internal_error(message: impl Into<String>) -> Self {
        Self {
            code: "InternalError",
            message: message.into(),
            http_status: StatusCode::INTERNAL_SERVER_ERROR,
            resource: String::new(),
        }
    }

    pub fn insufficient_storage(message: impl Into<String>) -> Self {
        Self {
            code: "InsufficientStorage",
            message: message.into(),
            http_status: StatusCode::INSUFFICIENT_STORAGE,
            resource: String::new(),
        }
    }

    pub fn slow_down(message: impl Into<String>) -> Self {
        Self {
            code: "SlowDown",
            message: message.into(),
            http_status: StatusCode::SERVICE_UNAVAILABLE,
            resource: String::new(),
        }
    }
}

impl IntoResponse for S3Error {
    fn into_response(self) -> Response {
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<Error>
  <Code>{}</Code>
  <Message>{}</Message>
  <Resource>{}</Resource>
  <RequestId>00000000-0000-0000-0000-000000000000</RequestId>
</Error>"#,
            self.code,
            crate::s3::xml::xml_escape(&self.message),
            crate::s3::xml::xml_escape(&self.resource),
        );

        (
            self.http_status,
            [("content-type", "application/xml")],
            xml,
        )
            .into_response()
    }
}
