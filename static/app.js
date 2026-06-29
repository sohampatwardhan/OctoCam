const copyButtons = document.querySelectorAll("[data-copy-target]");
const STREAM_PREVIEW_CACHE_KEY = "octocam.streamPreview";

if ("serviceWorker" in navigator) {
  window.addEventListener("load", () => {
    navigator.serviceWorker.register("/sw.js").catch(() => {});
  });
}

async function writeClipboard(text) {
  try {
    if (navigator.clipboard && window.isSecureContext) {
      await navigator.clipboard.writeText(text);
      return true;
    }
  } catch (error) {
  }

  const buffer = document.createElement("textarea");
  buffer.value = text;
  buffer.setAttribute("readonly", "");
  buffer.style.position = "fixed";
  buffer.style.inset = "0 auto auto 0";
  buffer.style.opacity = "0";
  document.body.appendChild(buffer);
  buffer.select();
  const copied = document.execCommand("copy");
  buffer.remove();
  return copied;
}

function selectTarget(target) {
  target.focus();
  target.select();
  target.setSelectionRange(0, target.value.length);
}

async function copyValue(button) {
  const target = document.getElementById(button.dataset.copyTarget);
  if (!target) {
    return;
  }

  const copied = await writeClipboard(target.value);
  if (!copied) {
    selectTarget(target);
  }

  button.dataset.copied = copied ? "true" : "selected";
  window.setTimeout(() => {
    delete button.dataset.copied;
  }, 1600);
}

copyButtons.forEach((button) => {
  button.addEventListener("click", () => copyValue(button));
});

const streamPreview = document.querySelector("[data-stream-preview]");

if (streamPreview) {
  const frame = streamPreview.querySelector("[data-stream-frame]");
  const placeholder = streamPreview.querySelector("[data-stream-placeholder]");
  const toggle = streamPreview.querySelector("[data-stream-toggle]");
  const choices = streamPreview.querySelectorAll("[data-stream-choice]");
  const sources = {
    main: streamPreview.dataset.mainSrc || "",
    sub: streamPreview.dataset.subSrc || "",
  };
  let activeStream = streamPreview.dataset.initialStream || "main";
  let playing = true;

  function loadPreviewCache() {
    try {
      const cached = JSON.parse(localStorage.getItem(STREAM_PREVIEW_CACHE_KEY) || "{}");
      if (cached.activeStream === "main" || (cached.activeStream === "sub" && sources.sub)) {
        activeStream = cached.activeStream;
      }
      if (typeof cached.playing === "boolean") {
        playing = cached.playing;
      }
    } catch (error) {
    }
  }

  function savePreviewCache() {
    try {
      localStorage.setItem(
        STREAM_PREVIEW_CACHE_KEY,
        JSON.stringify({ activeStream, playing }),
      );
    } catch (error) {
    }
  }

  function activeSource() {
    return sources[activeStream] || sources.main;
  }

  function syncPreview() {
    choices.forEach((choice) => {
      const selected = choice.dataset.streamChoice === activeStream;
      choice.setAttribute("aria-pressed", selected ? "true" : "false");
    });

    if (toggle) {
      toggle.textContent = playing ? "Stop" : "Start";
      toggle.setAttribute("aria-pressed", playing ? "true" : "false");
    }

    if (placeholder) {
      placeholder.hidden = playing;
    }

    if (!frame) {
      savePreviewCache();
      return;
    }

    if (playing) {
      const source = activeSource();
      if (frame.getAttribute("src") !== source) {
        frame.setAttribute("src", source);
      }
    } else {
      frame.setAttribute("src", "about:blank");
    }

    savePreviewCache();
  }

  choices.forEach((choice) => {
    choice.addEventListener("click", () => {
      if (choice.disabled) {
        return;
      }
      activeStream = choice.dataset.streamChoice || "main";
      playing = true;
      syncPreview();
    });
  });

  if (toggle) {
    toggle.addEventListener("click", () => {
      playing = !playing;
      syncPreview();
    });
  }

  loadPreviewCache();
  syncPreview();
}
