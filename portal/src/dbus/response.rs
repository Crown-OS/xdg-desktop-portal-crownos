/// The `response` every portal method answers with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Response {
    Success = 0,
    Cancelled = 1,
    Failed = 2,
}

impl From<Response> for u32 {
    fn from(response: Response) -> Self {
        response as Self
    }
}
