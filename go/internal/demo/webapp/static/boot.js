// Loads the Go WebAssembly client. External (not inline) so the CSP stays strict.
"use strict";
(async () => {
  const go = new Go();
  try {
    let result;
    if (WebAssembly.instantiateStreaming) {
      result = await WebAssembly.instantiateStreaming(fetch("app.wasm"), go.importObject);
    } else {
      const bytes = await (await fetch("app.wasm")).arrayBuffer();
      result = await WebAssembly.instantiate(bytes, go.importObject);
    }
    go.run(result.instance);
  } catch (err) {
    const app = document.getElementById("app");
    app.textContent = "Failed to load the web client: " + err;
  }
})();
