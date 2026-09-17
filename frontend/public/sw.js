/**
 * i-rs-schedule service worker:极简 PWA 壳层。
 * - /assets/*(带 hash 的构建产物):cache-first
 * - index.html / 导航请求:network-first,离线回落缓存
 * - /api/*(含 live/events 长轮询):永不缓存
 * 缓存名带版本,activate 时清理旧版本。
 */
const CACHE = "irs-shell-v1";
const SHELL = ["/", "/manifest.webmanifest", "/favicon.svg", "/icon-192.png", "/icon-512.png"];

self.addEventListener("install", (event) => {
  event.waitUntil(
    caches.open(CACHE).then((cache) => cache.addAll(SHELL)).then(() => self.skipWaiting()),
  );
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) => Promise.all(keys.filter((k) => k !== CACHE).map((k) => caches.delete(k))))
      .then(() => self.clients.claim()),
  );
});

self.addEventListener("fetch", (event) => {
  const url = new URL(event.request.url);
  if (event.request.method !== "GET" || url.origin !== self.location.origin) return;
  // API 与长轮询一律直连,不缓存
  if (url.pathname.startsWith("/api/")) return;

  // 带 hash 的静态资源:cache-first
  if (url.pathname.startsWith("/assets/")) {
    event.respondWith(
      caches.match(event.request).then(
        (hit) =>
          hit ||
          fetch(event.request).then((resp) => {
            const copy = resp.clone();
            caches.open(CACHE).then((cache) => cache.put(event.request, copy));
            return resp;
          }),
      ),
    );
    return;
  }

  // 导航与壳层:network-first,离线回落
  event.respondWith(
    fetch(event.request)
      .then((resp) => {
        const copy = resp.clone();
        caches.open(CACHE).then((cache) => cache.put(event.request, copy));
        return resp;
      })
      .catch(() => caches.match(event.request).then((hit) => hit || caches.match("/"))),
  );
});
