const api = globalThis.browser ?? globalThis.chrome;
const HOST_NAME = "singlepdf.host";
const CLEAR_BADGE_DELAY_MS = 5000;
const NOTIFICATION_ICON =
  "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 128 128'%3E%3Crect width='128' height='128' rx='24' fill='%231f4b99'/%3E%3Cpath d='M31 34h47c10.5 0 19 8.5 19 19v22c0 10.5-8.5 19-19 19H58l-17 16v-16h-10c-10.5 0-19-8.5-19-19V53c0-10.5 8.5-19 19-19Z' fill='white'/%3E%3Cpath d='M48 53h31v8H48zm0 16h22v8H48z' fill='%231f4b99'/%3E%3C/svg%3E";
const ACTIVE_RUNS = new Map();
const ACTION_STATES = {
  capturing: {
    badgeText: "CAP",
    badgeColor: "#1f4b99",
    title: "SinglePDF is capturing the page"
  },
  rendering: {
    badgeText: "PDF",
    badgeColor: "#7a4a00",
    title: "SinglePDF is rendering the PDF"
  },
  downloading: {
    badgeText: "DL",
    badgeColor: "#0b6b57",
    title: "SinglePDF is downloading the PDF"
  },
  success: {
    badgeText: "OK",
    badgeColor: "#0b6b57",
    title: "SinglePDF finished successfully"
  },
  busy: {
    badgeText: "...",
    badgeColor: "#5f6368",
    title: "SinglePDF is already running for this tab"
  },
  error: {
    badgeText: "ERR",
    badgeColor: "#b3261e",
    title: "SinglePDF failed"
  }
};

api.browserAction.onClicked.addListener(async (tab) => {
  if (!tab || typeof tab.id !== "number") {
    return;
  }

  if (ACTIVE_RUNS.has(tab.id)) {
    await indicateBusy(tab.id);
    return;
  }

  const runPromise = runCapture(tab).finally(() => {
    ACTIVE_RUNS.delete(tab.id);
  });
  ACTIVE_RUNS.set(tab.id, runPromise);

  await runPromise;
});

async function runCapture(tab) {
  try {
    assertSupportedTab(tab);
    await setActionState(tab.id, ACTION_STATES.capturing);
    await updateTabStatus(tab.id, {
      phase: "capturing",
      message: "SinglePDF is capturing this page..."
    });

    const page = await requestCapturedPage(tab.id);
    if (!page || !page.ok) {
      throw createStageError("capture", page?.error || "content capture failed", {
        code: page?.code || "capture_failed",
        details: page
      });
    }

    await setActionState(tab.id, ACTION_STATES.rendering);
    await updateTabStatus(tab.id, {
      phase: "rendering",
      message: "SinglePDF is rendering the PDF..."
    });
    const response = await sendNativeMessage({
      type: "render_snapshot",
      snapshot: page.snapshot
    });

    if (!response || !response.ok) {
      throw createStageError("native_host", response?.error || "native host failed", {
        code: response?.code || "native_host_failed",
        details: response
      });
    }

    await setActionState(tab.id, ACTION_STATES.downloading);
    await updateTabStatus(tab.id, {
      phase: "downloading",
      message: "SinglePDF is saving the PDF to Downloads..."
    });

    const renderReport = response.report || null;
    const skippedImages = Number(renderReport?.skipped_images || 0);
    const fallbackRecords = Number(renderReport?.fallback_records || 0);
    console.info("SinglePDF capture succeeded", {
      stage: "complete",
      tabId: tab.id,
      url: page.snapshot?.metadata?.url || tab.url || null,
      snapshotUrl: page.snapshot?.metadata?.url || null,
      filename: response.filename || "singlepdf-document.pdf",
      pageCount: response.page_count ?? null,
      savedPath: response.saved_path || null,
      reportPath: response.report_path || null,
      renderReport
    });
    await setActionState(tab.id, ACTION_STATES.success);
    const summaryBits = [];
    if (skippedImages > 0) {
      summaryBits.push(`${skippedImages} image${skippedImages === 1 ? "" : "s"} skipped`);
    }
    if (fallbackRecords > 0) {
      summaryBits.push(`${fallbackRecords} compression fallback${fallbackRecords === 1 ? "" : "s"}`);
    }
    if (response.report_path) {
      summaryBits.push("report saved");
    }
    const summary = summaryBits.length > 0 ? ` (${summaryBits.join(", ")})` : "";
    await updateTabStatus(tab.id, {
      phase: "success",
      message: `Saved ${response.filename || "singlepdf-document.pdf"} to Downloads${summary}`
    });
    scheduleBadgeClear(tab.id);
    scheduleTabStatusClear(tab.id);
  } catch (error) {
    const normalizedError = normalizeError(error, tab);
    console.error("SinglePDF capture failed", normalizedError);
    await setActionState(tab.id, ACTION_STATES.error, normalizedError.userMessage);
    scheduleBadgeClear(tab.id);
    await updateTabStatus(tab.id, {
      phase: "error",
      message: normalizedError.userMessage
    });
    scheduleTabStatusClear(tab.id, 8000);
    await showFailureNotification(normalizedError);
  }
}

function requestCapturedPage(tabId) {
  if (api.tabs.sendMessage.length <= 2) {
    return api.tabs.sendMessage(tabId, { type: "capture_page" }).catch((error) => {
      throw classifyRuntimeError("capture", error);
    });
  }
  return new Promise((resolve, reject) => {
    api.tabs.sendMessage(tabId, { type: "capture_page" }, (result) => {
      const error = api.runtime.lastError;
      if (error) {
        reject(classifyRuntimeError("capture", error));
        return;
      }
      resolve(result);
    });
  });
}

function sendNativeMessage(message) {
  if (api.runtime.sendNativeMessage.length <= 2) {
    return api.runtime.sendNativeMessage(HOST_NAME, message).catch((error) => {
      throw classifyRuntimeError("native_host", error);
    });
  }
  return new Promise((resolve, reject) => {
    api.runtime.sendNativeMessage(HOST_NAME, message, (response) => {
      const error = api.runtime.lastError;
      if (error) {
        reject(classifyRuntimeError("native_host", error));
        return;
      }
      resolve(response);
    });
  });
}

async function indicateBusy(tabId) {
  await setActionState(tabId, ACTION_STATES.busy);
  scheduleBadgeClear(tabId, 2000);
}

function assertSupportedTab(tab) {
  const url = tab?.url || "";
  if (
    url.startsWith("about:") ||
    url.startsWith("chrome:") ||
    url.startsWith("edge:") ||
    url.startsWith("chrome-extension:") ||
    url.startsWith("moz-extension:")
  ) {
    throw createStageError(
      "capture",
      "SinglePDF cannot capture this browser-internal page.",
      { code: "unsupported_page" }
    );
  }
}

async function setActionState(tabId, state, titleOverride) {
  await Promise.all([
    setBadgeText(tabId, state.badgeText),
    setBadgeBackgroundColor(tabId, state.badgeColor),
    setActionTitle(tabId, titleOverride || state.title)
  ]);
}

function scheduleBadgeClear(tabId, delayMs = CLEAR_BADGE_DELAY_MS) {
  setTimeout(() => {
    clearActionState(tabId).catch((error) => {
      console.warn("SinglePDF could not clear badge state", {
        tabId,
        error: error?.message || String(error)
      });
    });
  }, delayMs);
}

function scheduleTabStatusClear(tabId, delayMs = CLEAR_BADGE_DELAY_MS) {
  setTimeout(() => {
    updateTabStatus(tabId, { phase: "clear" }).catch((error) => {
      console.warn("SinglePDF could not clear tab status", {
        tabId,
        error: error?.message || String(error)
      });
    });
  }, delayMs);
}

async function clearActionState(tabId) {
  await Promise.all([
    setBadgeText(tabId, ""),
    setActionTitle(tabId, "Save page as PDF")
  ]);
}

function setBadgeText(tabId, text) {
  return callBrowserAction("setBadgeText", { tabId, text });
}

function setBadgeBackgroundColor(tabId, color) {
  return callBrowserAction("setBadgeBackgroundColor", { tabId, color });
}

function setActionTitle(tabId, title) {
  return callBrowserAction("setTitle", { tabId, title });
}

function callBrowserAction(methodName, details) {
  const method = api.browserAction?.[methodName];
  if (typeof method !== "function") {
    return Promise.resolve();
  }

  if (method.length <= 1) {
    return Promise.resolve(method.call(api.browserAction, details));
  }

  return new Promise((resolve, reject) => {
    method.call(api.browserAction, details, () => {
      const error = api.runtime.lastError;
      if (error) {
        reject(new Error(error.message));
        return;
      }
      resolve();
    });
  });
}

async function showFailureNotification(error) {
  if (!api.notifications?.create) {
    return;
  }

  try {
    const notificationId = await createNotification({
      type: "basic",
      iconUrl: NOTIFICATION_ICON,
      title: error.userTitle,
      message: error.userMessage
    });
    if (notificationId && typeof api.notifications.clear === "function") {
      setTimeout(() => {
        clearNotification(notificationId).catch(() => {});
      }, 10000);
    }
  } catch (notificationError) {
    console.warn("SinglePDF could not show failure notification", {
      error: notificationError?.message || String(notificationError)
    });
  }
}

function createNotification(options) {
  if (api.notifications.create.length <= 1) {
    return Promise.resolve(api.notifications.create(options));
  }

  return new Promise((resolve, reject) => {
    api.notifications.create(options, (notificationId) => {
      const error = api.runtime.lastError;
      if (error) {
        reject(new Error(error.message));
        return;
      }
      resolve(notificationId);
    });
  });
}

function clearNotification(notificationId) {
  if (api.notifications.clear.length <= 1) {
    return Promise.resolve(api.notifications.clear(notificationId));
  }

  return new Promise((resolve, reject) => {
    api.notifications.clear(notificationId, (wasCleared) => {
      const error = api.runtime.lastError;
      if (error) {
        reject(new Error(error.message));
        return;
      }
      resolve(wasCleared);
    });
  });
}

function updateTabStatus(tabId, payload) {
  if (!Number.isInteger(tabId)) {
    return Promise.resolve();
  }

  if (api.tabs.sendMessage.length <= 2) {
    return api.tabs
      .sendMessage(tabId, { type: "singlepdf_status", ...payload })
      .catch(() => undefined);
  }

  return new Promise((resolve) => {
    api.tabs.sendMessage(tabId, { type: "singlepdf_status", ...payload }, () => {
      resolve();
    });
  });
}

function classifyRuntimeError(stage, error) {
  const message = error?.message || String(error);
  const lower = message.toLowerCase();

  if (stage === "native_host") {
    if (lower.includes("native messaging host not found") || lower.includes("no such native application")) {
      return createStageError(stage, "SinglePDF native host is not installed for this browser.", {
        code: "native_host_missing",
        details: { message }
      });
    }
    if (lower.includes("native messaging host") && lower.includes("forbidden")) {
      return createStageError(
        stage,
        "SinglePDF native host is installed but this extension is not allowed to use it.",
        {
          code: "native_host_forbidden",
          details: { message }
        }
      );
    }
  }

  if (stage === "capture" && lower.includes("receiving end does not exist")) {
    return createStageError(
      stage,
      "SinglePDF could not reach the capture script in this tab. Reload the page and try again.",
      {
        code: "capture_unavailable",
        details: { message }
      }
    );
  }

  return createStageError(stage, message, {
    code: `${stage}_error`,
    details: { message }
  });
}

function createStageError(stage, message, extra = {}) {
  return {
    name: "SinglePdfExtensionError",
    stage,
    message,
    code: extra.code || `${stage}_failed`,
    details: extra.details || null
  };
}

function normalizeError(error, tab) {
  const stage = error?.stage || "unknown";
  const message = error?.message || String(error);
  const code = error?.code || `${stage}_failed`;
  const userTitle = getUserTitle(stage, code);
  const userMessage = getUserMessage(stage, code, message);

  return {
    stage,
    code,
    message,
    userTitle,
    userMessage,
    tabId: tab?.id ?? null,
    tabUrl: tab?.url ?? null,
    details: error?.details || null
  };
}

function getUserTitle(stage, code) {
  if (code === "native_host_missing") {
    return "SinglePDF native host missing";
  }
  if (code === "native_host_forbidden") {
    return "SinglePDF host registration mismatch";
  }
  if (code === "unsupported_page") {
    return "SinglePDF cannot run here";
  }
  if (stage === "capture") {
    return "SinglePDF capture failed";
  }
  if (stage === "native_host") {
    return "SinglePDF PDF conversion failed";
  }
  return "SinglePDF failed";
}

function getUserMessage(stage, code, message) {
  if (code === "native_host_missing") {
    return "Install the native host for this browser, then reload the extension and try again.";
  }
  if (code === "native_host_forbidden") {
    return "The native host manifest does not allow this extension ID. Reinstall the host registration and try again.";
  }
  if (code === "unsupported_page") {
    return message;
  }
  if (code === "capture_unavailable") {
    return message;
  }
  if (stage === "capture") {
    return `Page capture failed: ${message}`;
  }
  if (stage === "native_host") {
    return `PDF generation failed: ${message}`;
  }
  return message;
}
