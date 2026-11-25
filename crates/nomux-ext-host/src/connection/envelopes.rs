//! The envelopes that the host sends.

use nomux_extension::wire::v1::envelope::Kind;
use nomux_extension::wire::v1::request::Body;
use nomux_extension::wire::v1::{Cancel, Envelope, Request, Response, response};

pub(super) fn request(id: u64, body: Body) -> Envelope {
    let request = Request {
        id,
        body: Some(body),
    };
    Envelope {
        kind: Some(Kind::Request(request)),
    }
}

pub(super) fn response(id: u64, result: response::Result) -> Envelope {
    let response = Response {
        id,
        result: Some(result),
    };
    Envelope {
        kind: Some(Kind::Response(response)),
    }
}

pub(super) fn cancel(id: u64) -> Envelope {
    Envelope {
        kind: Some(Kind::Cancel(Cancel { id })),
    }
}
