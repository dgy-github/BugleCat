//! Live rust-analyzer acceptance for the production LSP provider.

use ncx_core::{LspProvider, LspRequest, RustAnalyzerProvider};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let workspace = std::env::current_dir()
        .expect("current directory")
        .join("rust")
        .canonicalize()
        .expect("rust workspace");
    let provider = RustAnalyzerProvider::new(workspace);
    let result = provider
        .request(LspRequest {
            operation: "document_symbols".to_string(),
            path: Some("crates/ncx-core/src/lib.rs".to_string()),
            line: None,
            column: None,
            query: None,
        })
        .await
        .expect("rust-analyzer document symbols");
    let count = result.as_array().map_or(0, Vec::len);
    assert!(
        count > 0,
        "rust-analyzer returned no document symbols: {result}"
    );
    println!("lsp document_symbols: ok ({count} symbols)");
}
