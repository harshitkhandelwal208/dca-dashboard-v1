//! DCA core: local screenshot reading (PaddleOCR / PP-OCR on ONNX Runtime), standings and driver-licence
//! parsing, spreadsheet + report export and image rendering. Everything runs locally: no network, no API keys.

pub mod calc;
pub mod emoji;
pub mod imaging;
pub mod licence;
pub mod metrics;
pub mod names;
pub mod ocr;
pub mod photo;
pub mod reader;
pub mod render;
pub mod report;
pub mod session;
pub mod standings;
pub mod unicode_names;
