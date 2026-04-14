pub mod compression;
pub mod dom;
pub mod encode;
pub mod model;
pub mod native_host;
pub mod paginate;
pub mod pdf;
pub mod profile;
pub mod snapshot;
pub mod text_layout;

pub use dom::DomProcessor;
pub use model::{
    BlockKind, Color, FlowBlock, LogicalPage, MarginBox, PageLayout, ParsedDocument, Rect,
    RenderPage, RenderReport, SnapshotDocument, SnapshotMetadata, TableRole, TextStyle,
};
pub use paginate::Paginator;
pub use pdf::PdfRenderer;
pub use profile::SinglePdfProfile;
pub use snapshot::HtmlSnapshot;
