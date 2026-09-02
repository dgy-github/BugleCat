import assert from "node:assert/strict";
import test from "node:test";

import {
  checkGeneratedProtocolModule,
  extractProtocolMethods,
  extractProtocolVersion,
  renderProtocolModule,
} from "./check-protocol-version.mjs";

test("extracts the single Rust protocol version declaration", () => {
  assert.equal(
    extractProtocolVersion("pub const PROTOCOL_VERSION: u32 = 17;"),
    17,
  );
});

test("rejects missing or duplicate Rust declarations", () => {
  assert.throws(() => extractProtocolVersion("const PROTOCOL_VERSION: u32 = 3;"));
  assert.throws(() =>
    extractProtocolVersion(
      "pub const PROTOCOL_VERSION: u32 = 3;\npub const PROTOCOL_VERSION: u32 = 4;",
    ),
  );
});

test("extracts top-level ClientRequest methods in serde casing", () => {
  const source = `
pub enum ClientRequest {
    ThreadRead { thread_id: String },
    RuntimeStatusRead,
    DshMarketplaceSearch {
        query: String,
    },
}
`;
  assert.deepEqual(extractProtocolMethods(source), [
    "threadRead",
    "runtimeStatusRead",
    "dshMarketplaceSearch",
  ]);
});

test("honors an explicit serde method rename", () => {
  const source = `
pub enum ClientRequest {
    #[serde(rename = "legacy-status")]
    RuntimeStatusRead,
}
`;
  assert.deepEqual(extractProtocolMethods(source), ["legacy-status"]);
});

test("detects generated TypeScript drift", () => {
  const methods = ["threadRead"];
  const current = renderProtocolModule(3, methods).replace("= 3", "= 2");
  assert.throws(
    () => checkGeneratedProtocolModule(current, 3, methods),
    /out of date/,
  );
  assert.doesNotThrow(() =>
    checkGeneratedProtocolModule(
      renderProtocolModule(3, methods).replaceAll("\n", "\r\n"),
      3,
      methods,
    ),
  );
});
