const api = globalThis.browser ?? globalThis.chrome;
const OVERLAY_ID = "__singlepdf_status_overlay__";
const OVERLAY_STYLE_ID = "__singlepdf_status_overlay_style__";
const CAPTURE_TIMEOUT_MS = 60000;

api.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (message?.type === "singlepdf_status") {
    handleStatusMessage(message);
    return undefined;
  }

  if (!message || message.type !== "capture_page") {
    return undefined;
  }

  capturePageWithSingleFile()
    .then((result) => {
      sendResponse({ ok: true, ...result });
    })
    .catch((error) => {
      sendResponse({
        ok: false,
        stage: "capture",
        code: error?.code || "capture_failed",
        error: error?.message || String(error)
      });
    });

  return true;
});

async function capturePageWithSingleFile() {
  if (!globalThis.extension || typeof globalThis.extension.getPageData !== "function") {
    throw createCaptureError(
      "singlefile_runtime_missing",
      "SingleFile runtime is not available in the page context"
    );
  }

  const pageData = await withTimeout(
    globalThis.extension.getPageData({
      removeHiddenElements: true,
      removeUnusedStyles: true,
      removeUnusedFonts: true,
      removeImports: true,
      removeAlternativeFonts: true,
      removeAlternativeMedias: true,
      removeAlternativeImages: true,
      removeFrames: false,
      compressHTML: true,
      compressCSS: false,
      loadDeferredImages: true,
      loadDeferredImagesMaxIdleTime: 1500,
      groupDuplicateImages: true,
      blockScripts: true,
      blockVideos: true,
      blockAudios: true,
      saveRawPage: false,
      saveOriginalURLs: true,
      insertMetaCSP: true,
      insertSingleFileComment: true,
      displayStats: false,
      compressContent: false
    }),
    CAPTURE_TIMEOUT_MS,
    "singlefile_capture_timeout",
    "SingleFile capture timed out after 60 seconds"
  );

  if (!pageData || typeof pageData.content !== "string" || pageData.content.length === 0) {
    throw createCaptureError("singlefile_empty_capture", "SingleFile returned an empty page snapshot");
  }

  return {
    snapshot: {
      metadata: {
        title: pageData.title || document.title || null,
        url: location.href
      },
      html: pageData.content,
      hints: []
    }
  };
}

function createCaptureError(code, message) {
  const error = new Error(message);
  error.code = code;
  return error;
}

function withTimeout(promise, timeoutMs, code, message) {
  return Promise.race([
    promise,
    new Promise((_, reject) => {
      setTimeout(() => reject(createCaptureError(code, message)), timeoutMs);
    })
  ]);
}

function handleStatusMessage(message) {
  if (message.phase === "clear") {
    clearStatusOverlay();
    return;
  }

  showStatusOverlay(message.phase, message.message || "SinglePDF is working...");
}

function showStatusOverlay(phase, message) {
  ensureOverlayStyle();

  let overlay = document.getElementById(OVERLAY_ID);
  if (!overlay) {
    overlay = document.createElement("div");
    overlay.id = OVERLAY_ID;
    overlay.innerHTML =
      '<div class="singlepdf-status-dot"></div><div class="singlepdf-status-text"></div>';
    document.documentElement.appendChild(overlay);
  }

  overlay.dataset.phase = phase || "working";
  const textNode = overlay.querySelector(".singlepdf-status-text");
  if (textNode) {
    textNode.textContent = message;
  }
}

function clearStatusOverlay() {
  document.getElementById(OVERLAY_ID)?.remove();
}

function ensureOverlayStyle() {
  if (document.getElementById(OVERLAY_STYLE_ID)) {
    return;
  }

  const style = document.createElement("style");
  style.id = OVERLAY_STYLE_ID;
  style.textContent = `
    #${OVERLAY_ID} {
      position: fixed;
      left: 16px;
      bottom: 16px;
      z-index: 2147483647;
      display: flex;
      align-items: center;
      gap: 10px;
      max-width: min(420px, calc(100vw - 32px));
      padding: 12px 14px;
      border-radius: 12px;
      background: rgba(20, 24, 33, 0.92);
      color: #f5f7fb;
      font: 13px/1.35 -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
      box-shadow: 0 10px 30px rgba(0, 0, 0, 0.28);
      backdrop-filter: blur(8px);
    }

    #${OVERLAY_ID} .singlepdf-status-dot {
      width: 10px;
      height: 10px;
      border-radius: 999px;
      background: #7aa2ff;
      flex: 0 0 auto;
      box-shadow: 0 0 0 0 rgba(122, 162, 255, 0.45);
      animation: singlepdf-pulse 1.4s infinite;
    }

    #${OVERLAY_ID}[data-phase="rendering"] .singlepdf-status-dot {
      background: #ffb454;
      box-shadow: 0 0 0 0 rgba(255, 180, 84, 0.45);
    }

    #${OVERLAY_ID}[data-phase="downloading"] .singlepdf-status-dot {
      background: #4fd1a5;
      box-shadow: 0 0 0 0 rgba(79, 209, 165, 0.45);
    }

    #${OVERLAY_ID}[data-phase="success"] .singlepdf-status-dot {
      background: #4fd1a5;
      animation: none;
    }

    #${OVERLAY_ID}[data-phase="error"] .singlepdf-status-dot {
      background: #ff7b72;
      animation: none;
    }

    #${OVERLAY_ID} .singlepdf-status-text {
      overflow-wrap: anywhere;
    }

    @keyframes singlepdf-pulse {
      0% {
        transform: scale(0.95);
        box-shadow: 0 0 0 0 currentColor;
      }
      70% {
        transform: scale(1);
        box-shadow: 0 0 0 10px transparent;
      }
      100% {
        transform: scale(0.95);
        box-shadow: 0 0 0 0 transparent;
      }
    }
  `;
  document.documentElement.appendChild(style);
}
