//! pdfr core: geometry, font, field model, text layout and editing, reading
//! real fields, detecting fields, saving, and the rendering wrapper. No UI.

pub mod demo;
pub mod detect;
pub mod digital;
pub mod edit;
pub mod field;
pub mod flatten;
pub mod font;
pub mod forms;
pub mod geom;
pub mod layout;
pub mod links;
pub mod markup;
pub mod outline;
pub mod pages;
pub mod pdf_font;
pub mod pipeline;
pub mod redact;
pub mod render;
pub mod save;
pub mod search;
pub mod signature;
pub mod text;
pub mod xfa;

/// The most bytes one stream may decompress to (64 MiB). A small file can
/// hold a stream that inflates to gigabytes; anything over this is refused.
pub const MAX_STREAM_BYTES: usize = 64 << 20;
