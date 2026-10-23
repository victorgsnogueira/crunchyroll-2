// Crunchyroll Desktop player page. Talks to Rust through window.ipc and the
// /proxy/* routes served by the app (manifest, DRM license, subtitle files).
"use strict";

const $ = (id) => document.getElementById(id);
const video = $("video");
const host = (msg) => { try { window.ipc.postMessage(JSON.stringify(msg)); } catch (_) {} };
const log = (m) => host({ type: "log", message: String(m) });
window.addEventListener("error", (e) => log(`erro JS: ${e.message} @ ${e.filename}:${e.lineno}`));
window.addEventListener("unhandledrejection", (e) => log(`promise rejeitada: ${e.reason && (e.reason.message || e.reason)}`));

const KEY_SYSTEMS = {
  widevine: ["com.widevine.alpha"],
  playready: ["com.microsoft.playready.recommendation", "com.microsoft.playready"],
};
const SKIP_LABEL = { intro: "Pular abertura", recap: "Pular recapitulação", credits: "Pular encerramento", preview: "Pular prévia" };

const state = {
  info: null, player: null, jassub: null, subTrack: null, subsOn: true,
  idleTimer: 0, nextTimer: 0, nextDismissed: false, lastReport: 0, debug: false,
  keySystem: "", drmOrder: ["widevine", "playready"], dragging: false, menu: "",
};

const clock = (s) => {
  s = Math.max(0, Math.floor(s || 0));
  const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), x = s % 60;
  return h ? `${h}:${String(m).padStart(2, "0")}:${String(x).padStart(2, "0")}` : `${m}:${String(x).padStart(2, "0")}`;
};

// ---------------------------------------------------------------- DRM probe

async function probeKeySystems() {
  const config = [{
    initDataTypes: ["cenc"],
    videoCapabilities: [{ contentType: 'video/mp4; codecs="avc1.640028"' }],
    audioCapabilities: [{ contentType: 'audio/mp4; codecs="mp4a.40.2"' }],
  }];
  const found = {};
  for (const [name, ids] of Object.entries(KEY_SYSTEMS)) {
    for (const id of ids) {
      try { await navigator.requestMediaKeySystemAccess(id, config); found[name] = id; break; } catch (_) {}
    }
  }
  return found;
}

// -------------------------------------------------------------- load video

async function load(info, opts) {
  state.info = info;
  state.debug = !!(opts && opts.debug);
  if (opts && opts.drmOrder) state.drmOrder = opts.drmOrder;
  state.nextDismissed = false;
  $("debug").classList.toggle("hidden", !state.debug);
  $("t-title").textContent = info.title || "";
  $("t-sub").textContent = info.subtitle || "";
  $("btn-next").classList.toggle("hidden", !info.next);
  $("error").classList.add("hidden");
  $("loading").classList.remove("off");
  hideNext();
  renderMarkers();

  try {
    const found = await probeKeySystems();
    host({ type: "keysystems", found });
    const order = state.drmOrder.filter((n) => found[n]);
    if (!order.length) throw new Error("Este computador não tem um módulo de DRM disponível no WebView2 (Widevine/PlayReady).");

    shaka.polyfill.installAll();
    if (state.player) { await state.player.destroy(); }
    const player = new shaka.Player();
    await player.attach(video);
    state.player = player;

    const servers = {};
    for (const n of order) servers[found[n]] = `/proxy/license?system=${n}`;
    player.configure({
      drm: { servers, preferredKeySystems: order.map((n) => found[n]) },
      streaming: { bufferingGoal: 40, rebufferingGoal: 2, bufferBehind: 30 },
      abr: { enabled: true, defaultBandwidthEstimate: 8_000_000 },
      preferredAudioLanguage: "",
    });
    // CDN segments go through the app (the CDN rejects web-page origins).
    player.getNetworkingEngine().registerRequestFilter((type, request) => {
      if (type === shaka.net.NetworkingEngine.RequestType.SEGMENT) {
        request.uris = request.uris.map((u) =>
          u.startsWith("https://") ? `${location.origin}/proxy/seg?u=${encodeURIComponent(u)}` : u);
      }
    });
    player.addEventListener("error", (e) => fail(e.detail));
    player.addEventListener("adaptation", reportStats);
    player.addEventListener("variantchanged", reportStats);

    await player.load("/proxy/manifest", info.resume_at || 0, "application/dash+xml");
    state.keySystem = player.keySystem() || "";
    const heights = player.getVariantTracks().map((t) => t.height || 0);
    host({ type: "loaded", keySystem: state.keySystem, maxHeight: Math.max(0, ...heights) });

    await setupSubtitles();
    restoreVolume();
    await video.play().catch(() => {});
    $("loading").classList.add("off");
    wake();
  } catch (err) {
    fail(err);
  }
}

function fail(err) {
  const code = err && err.code ? ` (código ${err.code})` : "";
  let msg = (err && (err.message || (err.data && err.data.join && err.data.join(" ")))) || String(err);
  if (err && err.code >= 6000 && err.code < 7000) msg = "O servidor de licença recusou a reprodução neste aparelho." + code;
  else if (err && err.code) msg = `Erro do player${code}.`;
  $("error-msg").textContent = msg;
  $("error").classList.remove("hidden");
  $("loading").classList.add("off");
  // technical detail goes to the app log (URL, HTTP status, response start)
  let detail = "";
  try {
    detail = JSON.stringify((err && err.data) || [], (k, v) => {
      if (v instanceof ArrayBuffer) return new TextDecoder().decode(v.slice(0, 300));
      if (ArrayBuffer.isView(v)) return new TextDecoder().decode(v.buffer.slice(0, 300));
      return v;
    }).slice(0, 1500);
  } catch (_) {}
  host({ type: "error", message: `${msg} ${detail}`, code: (err && err.code) || 0 });
}

// --------------------------------------------------------------- subtitles

async function setupSubtitles() {
  if (state.jassub) { try { state.jassub.destroy(); } catch (_) {} state.jassub = null; }
  const subs = state.info.subtitles || [];
  const pref = state.info.preferred_subtitle;
  let pick = state.subTrack && subs.find((s) => s.locale === state.subTrack.locale);
  if (!pick && pref) pick = subs.find((s) => s.locale === pref);
  state.subTrack = pick || null;
  log(`legendas: ${subs.map((x) => x.locale + "/" + x.format).join(", ") || "nenhuma"}; escolhida: ${state.subTrack ? state.subTrack.locale : "nenhuma"}`);
  if (state.subTrack && state.subsOn) await applySubtitle(state.subTrack);
  $("btn-subs").classList.toggle("on", !!(state.subTrack && state.subsOn));
}

async function applySubtitle(track) {
  if (state.jassub) { try { state.jassub.destroy(); } catch (_) {} state.jassub = null; }
  try { state.player && state.player.setTextTrackVisibility(false); } catch (_) {}
  if (!track) return;
  const url = `/proxy/text?u=${encodeURIComponent(track.url)}`;
  if (track.format === "ass" || track.format === "ssa") {
    state.jassub = new JASSUB({
      video,
      subUrl: url,
      workerUrl: "/vendor/jassub-worker.js",
      wasmUrl: "/vendor/jassub-worker.wasm",
      modernWasmUrl: "/vendor/jassub-worker-modern.wasm",
      fallbackFont: "lato",
      availableFonts: { lato: "/fonts/Lato-Bold.ttf", "liberation sans": "/fonts/Lato-Bold.ttf" },
      prescaleFactor: 1,
      asyncRender: true,
    });
    try {
      state.jassub.addEventListener("ready", () => {
        log("JASSUB pronto");
      });
      state.jassub.addEventListener("error", (e) => log(`JASSUB erro: ${e && (e.message || e.error || e.type)}`));
    } catch (e) { log(`JASSUB eventos: ${e.message}`); }
  } else {
    await state.player.addTextTrackAsync(url, track.locale, "subtitle", "text/vtt");
    const t = state.player.getTextTracks().find((x) => x.language === track.locale);
    if (t) { state.player.selectTextTrack(t); state.player.setTextTrackVisibility(true); }
  }
}

function toggleSubs() {
  const subs = state.info ? state.info.subtitles : [];
  if (!subs.length) return;
  state.subsOn = !state.subsOn;
  if (!state.subTrack) state.subTrack = subs.find((s) => s.locale === "pt-BR") || subs[0];
  applySubtitle(state.subsOn ? state.subTrack : null);
  $("btn-subs").classList.toggle("on", state.subsOn);
}

// ---------------------------------------------------------------- controls

function togglePlay() { if (video.paused) video.play(); else video.pause(); wake(); }
function seekBy(d) { video.currentTime = Math.max(0, Math.min(video.duration || 0, video.currentTime + d)); wake(); }
function setVolume(v) {
  video.volume = Math.max(0, Math.min(1, v));
  video.muted = video.volume === 0;
  $("vol").value = video.volume;
  try { localStorage.setItem("vol", String(video.volume)); } catch (_) {}
  updateVolumeIcon();
}
function restoreVolume() {
  let v = 1;
  try { const s = localStorage.getItem("vol"); if (s !== null) v = parseFloat(s); } catch (_) {}
  setVolume(isNaN(v) ? 1 : v);
}
function updateVolumeIcon() {
  const m = video.muted || video.volume === 0;
  $("ic-x").classList.toggle("hidden", !m);
  $("ic-wave1").classList.toggle("hidden", m);
  $("ic-wave2").classList.toggle("hidden", m || video.volume < 0.5);
}

function activeSkip() {
  const t = video.currentTime;
  return (state.info && state.info.skip || []).find((m) => t >= m.start && t < m.end - 1);
}
function doSkip() { const m = activeSkip(); if (m) { video.currentTime = m.end; wake(); } }

function renderMarkers() {
  const box = $("bar-markers");
  box.innerHTML = "";
  const d = video.duration;
  if (!state.info || !d || !isFinite(d)) return;
  for (const m of state.info.skip || []) {
    if (m.kind !== "intro" && m.kind !== "credits") continue;
    const el = document.createElement("div");
    el.className = "bar-marker";
    el.style.left = `${(m.start / d) * 100}%`;
    el.style.width = `${((m.end - m.start) / d) * 100}%`;
    box.appendChild(el);
  }
}

// ------------------------------------------------------------- next episode

function maybeShowNext() {
  const info = state.info;
  if (!info || !info.next || state.nextDismissed || !video.duration) return;
  const credits = (info.skip || []).find((m) => m.kind === "credits");
  const start = credits ? credits.start : video.duration - 15;
  if (video.currentTime >= start && $("nextcard").classList.contains("hidden")) showNext();
}
function showNext() {
  const n = state.info.next;
  $("nc-title").textContent = n.title;
  $("nc-img").src = n.thumb || "";
  $("nextcard").classList.remove("hidden");
  $("skip").classList.add("hidden");
  const total = 8000, t0 = performance.now();
  const ring = $("nc-ring");
  clearInterval(state.nextTimer);
  state.nextTimer = setInterval(() => {
    const p = Math.min(1, (performance.now() - t0) / total);
    ring.style.strokeDashoffset = String(100.5 * p);
    if (p >= 1) { clearInterval(state.nextTimer); host({ type: "next" }); }
  }, 50);
}
function hideNext() { clearInterval(state.nextTimer); $("nextcard").classList.add("hidden"); }

// ------------------------------------------------------------- settings menu

function openMenu(kind) {
  const menu = $("menu");
  if (state.menu === kind) { closeMenu(); return; }
  state.menu = kind;
  const check = '<svg class="check" viewBox="0 0 24 24"><path d="M5 12l5 5 9-10"/></svg>';
  const item = (label, sel, action) => `<button class="menu-item${sel ? " sel" : ""}" data-a="${action}">${label}${check}</button>`;
  let html = "";
  const info = state.info || {};

  if (kind === "subs" || kind === "settings") {
    if ((info.audio_versions || []).length > 1) {
      html += '<div class="menu-h">Áudio</div>';
      for (const a of info.audio_versions) html += item(a.label, a.current, `audio:${a.media_id}`);
      html += '<div class="menu-sep"></div>';
    }
    html += '<div class="menu-h">Legendas</div>';
    html += item("Desligadas", !state.subsOn || !state.subTrack, "sub:off");
    for (const s of info.subtitles || []) html += item(s.label, state.subsOn && state.subTrack && state.subTrack.locale === s.locale, `sub:${s.locale}`);
  }
  if (kind === "settings" && state.player) {
    html += '<div class="menu-sep"></div><div class="menu-h">Qualidade</div>';
    const abr = state.player.getConfiguration().abr.enabled;
    const active = state.player.getVariantTracks().find((t) => t.active);
    const heights = [...new Set(state.player.getVariantTracks().map((t) => t.height).filter(Boolean))].sort((a, b) => b - a);
    html += item(`Automática${abr && active ? ` · ${active.height}p` : ""}`, abr, "q:auto");
    for (const h of heights) html += item(`${h}p${h >= 1080 ? " HD" : ""}`, !abr && active && active.height === h, `q:${h}`);
    html += '<div class="menu-sep"></div><div class="menu-h">Velocidade</div>';
    for (const r of [0.5, 0.75, 1, 1.25, 1.5, 2]) html += item(r === 1 ? "Normal" : `${r}x`, video.playbackRate === r, `rate:${r}`);
  }
  menu.innerHTML = html;
  menu.classList.remove("hidden");
}
function closeMenu() { state.menu = ""; $("menu").classList.add("hidden"); }

$("menu").addEventListener("click", (e) => {
  const b = e.target.closest(".menu-item");
  if (!b) return;
  const [k, v] = b.dataset.a.split(/:(.*)/s);
  if (k === "audio") { host({ type: "audio", media_id: v, position: video.currentTime }); closeMenu(); return; }
  if (k === "sub") {
    if (v === "off") { state.subsOn = false; applySubtitle(null); }
    else { state.subsOn = true; state.subTrack = state.info.subtitles.find((s) => s.locale === v); applySubtitle(state.subTrack); }
    $("btn-subs").classList.toggle("on", state.subsOn);
  }
  if (k === "q") {
    if (v === "auto") state.player.configure({ abr: { enabled: true } });
    else {
      state.player.configure({ abr: { enabled: false } });
      const active = state.player.getVariantTracks().find((t) => t.active);
      const cands = state.player.getVariantTracks().filter((t) => t.height === Number(v) && (!active || t.language === active.language));
      cands.sort((a, b) => b.bandwidth - a.bandwidth);
      if (cands[0]) state.player.selectVariantTrack(cands[0], true);
    }
  }
  if (k === "rate") video.playbackRate = Number(v);
  const kind = state.menu; state.menu = ""; openMenu(kind);
});

// ------------------------------------------------------------------ seekbar

const bar = $("bar");
function barFraction(e) { const r = bar.getBoundingClientRect(); return Math.max(0, Math.min(1, (e.clientX - r.left) / r.width)); }
bar.addEventListener("mousemove", (e) => {
  const f = barFraction(e), r = bar.getBoundingClientRect();
  $("bar-tip").textContent = clock(f * (video.duration || 0));
  $("bar-tip").style.left = `${Math.max(30, Math.min(r.width - 30, f * r.width))}px`;
  if (state.dragging) paintProgress(f);
});
bar.addEventListener("mousedown", (e) => { state.dragging = true; bar.classList.add("drag"); paintProgress(barFraction(e)); });
window.addEventListener("mouseup", (e) => {
  if (!state.dragging) return;
  state.dragging = false; bar.classList.remove("drag");
  if (video.duration) video.currentTime = barFraction(e) * video.duration;
});
function paintProgress(f) { $("bar-played").style.width = `${f * 100}%`; $("bar-knob").style.left = `${f * 100}%`; }

// -------------------------------------------------------------------- loop

video.addEventListener("timeupdate", () => {
  const d = video.duration || 0, t = video.currentTime;
  if (!state.dragging && d) paintProgress(t / d);
  $("t-cur").textContent = clock(t);
  const m = activeSkip();
  const sk = $("skip");
  if (m && $("nextcard").classList.contains("hidden")) {
    sk.firstChild.textContent = (SKIP_LABEL[m.kind] || "Pular") + " ";
    sk.classList.remove("hidden");
  } else sk.classList.add("hidden");
  maybeShowNext();
  const now = performance.now();
  if (now - state.lastReport > 5000) { state.lastReport = now; host({ type: "time", pos: t, dur: d, paused: video.paused }); }
});
video.addEventListener("durationchange", () => { $("t-dur").textContent = clock(video.duration); renderMarkers(); });
video.addEventListener("progress", () => {
  const b = video.buffered, d = video.duration;
  if (b.length && d) $("bar-buffer").style.width = `${(b.end(b.length - 1) / d) * 100}%`;
});
video.addEventListener("play", () => { $("ic-play").classList.add("hidden"); $("ic-pause").classList.remove("hidden"); wake(); });
video.addEventListener("pause", () => {
  $("ic-play").classList.remove("hidden"); $("ic-pause").classList.add("hidden"); wake(true);
  host({ type: "time", pos: video.currentTime, dur: video.duration || 0, paused: true });
});
video.addEventListener("waiting", () => $("loading").classList.remove("off"));
video.addEventListener("playing", () => $("loading").classList.add("off"));
video.addEventListener("ended", () => { host({ type: "time", pos: video.duration, dur: video.duration, paused: true }); host({ type: "ended" }); });
video.addEventListener("volumechange", updateVolumeIcon);

function reportStats() {
  const p = state.player;
  if (!p) return;
  const active = p.getVariantTracks().find((t) => t.active) || {};
  const st = p.getStats();
  const msg = { type: "stats", height: video.videoHeight, width: video.videoWidth, trackHeight: active.height || 0,
    bandwidth: active.bandwidth || 0, keySystem: p.keySystem() || "", dropped: st.droppedFrames || 0 };
  host(msg);
  if (state.debug) $("debug").textContent = `${msg.keySystem}\n${msg.width}x${msg.height} (faixa ${msg.trackHeight}p)\n${(msg.bandwidth / 1e6).toFixed(1)} Mb/s · ${msg.dropped} quadros perdidos`;
}
setInterval(reportStats, 3000);

// ---------------------------------------------------------------- idle UI

function wake(stay) {
  document.body.classList.remove("idle");
  $("ui").classList.add("visible");
  clearTimeout(state.idleTimer);
  if (!stay && !video.paused) {
    state.idleTimer = setTimeout(() => {
      if (state.menu || state.dragging || video.paused) return;
      $("ui").classList.remove("visible");
      document.body.classList.add("idle");
    }, 2600);
  }
}
document.addEventListener("mousemove", () => wake());

// ---------------------------------------------------------------- wiring

$("btn-play").onclick = togglePlay;
$("btn-rew").onclick = () => seekBy(-10);
$("btn-fwd").onclick = () => seekBy(10);
$("btn-next").onclick = () => host({ type: "next" });
$("btn-mute").onclick = () => { video.muted = !video.muted; if (!video.muted && video.volume === 0) setVolume(0.5); updateVolumeIcon(); };
$("vol").oninput = (e) => setVolume(parseFloat(e.target.value));
$("btn-subs").onclick = () => openMenu("subs");
$("btn-settings").onclick = () => openMenu("settings");
$("btn-full").onclick = () => host({ type: "fullscreen" });
$("btn-back").onclick = () => host({ type: "back", pos: video.currentTime });
$("error-back").onclick = () => host({ type: "back", pos: 0 });
$("skip").onclick = doSkip;
$("nc-play").onclick = () => host({ type: "next" });
$("nc-cancel").onclick = () => { state.nextDismissed = true; hideNext(); };
// Clicking the video toggles playback; with a menu open, the first click just closes it.
$("stage").addEventListener("click", () => { if (state.menu) closeMenu(); else togglePlay(); });
// Any click outside the menu (and outside the buttons that open it) closes it.
document.addEventListener("click", (e) => {
  if (!state.menu) return;
  if (e.target.closest("#menu, #btn-subs, #btn-settings, #stage")) return;
  closeMenu();
}, true);
// Buttons never keep keyboard focus, so Space/Enter never "re-press" them.
document.addEventListener("mousedown", (e) => { if (e.target.closest("button")) e.preventDefault(); });
document.querySelectorAll("button").forEach((b) => b.setAttribute("tabindex", "-1"));
$("stage").addEventListener("dblclick", () => host({ type: "fullscreen" }));

document.addEventListener("keydown", (e) => {
  if (e.target && e.target.tagName === "INPUT" && e.key !== "Escape") return;
  const k = e.key.toLowerCase();
  const handled = true;
  switch (k) {
    case " ": case "k": togglePlay(); break;
    case "arrowleft": case "j": seekBy(-10); break;
    case "arrowright": case "l": seekBy(10); break;
    case "arrowup": setVolume(video.volume + 0.05); break;
    case "arrowdown": setVolume(video.volume - 0.05); break;
    case "f": host({ type: "fullscreen" }); break;
    case "m": $("btn-mute").click(); break;
    case "s": doSkip(); break;
    case "n": if (state.info && state.info.next) host({ type: "next" }); break;
    case "c": toggleSubs(); break;
    case "d": state.debug = !state.debug; $("debug").classList.toggle("hidden", !state.debug); reportStats(); break;
    case "escape": if (state.menu) closeMenu(); else host({ type: "escape", pos: video.currentTime }); break;
    default: return;
  }
  if (handled) { e.preventDefault(); wake(); }
});

window.crPlayer = {
  load,
  pause: () => video.pause(),
  play: () => video.play(),
  position: () => video.currentTime,
  stop: async () => {
    video.pause();
    if (state.jassub) { try { state.jassub.destroy(); } catch (_) {} state.jassub = null; }
    if (state.player) { await state.player.unload(); }
  },
};
host({ type: "ready" });
