// Todoke — paper edition. Theme, smooth scroll, restrained reveals.
// Motion rules per the design system: reveals enhance already-visible
// content (no class-gated hiding), stagger ≤ 80 ms, reduced-motion = fades only.

// ---------------------------------------------------------------- theme
const root = document.documentElement;
const themeBtn = document.querySelector("[data-theme-toggle]");

function applyTheme(theme) {
  root.dataset.theme = theme;
  try { localStorage.setItem("todoke-theme", theme); } catch (e) { /* private mode */ }
}

themeBtn?.addEventListener("click", () => {
  applyTheme(root.dataset.theme === "dark" ? "light" : "dark");
});

// ---------------------------------------------------------------- year
const year = document.getElementById("year");
if (year) year.textContent = String(new Date().getFullYear());

// ---------------------------------------------------------------- reduced motion
const reduceMotion = matchMedia("(prefers-reduced-motion: reduce)");

// ---------------------------------------------------------------- lenis smooth scroll
let lenis = null;
async function initLenis() {
  const automation = navigator.webdriver || new URLSearchParams(location.search).has("shot");
  if (reduceMotion.matches || automation) return;
  try {
    const { default: Lenis } = await import("../vendor/lenis/lenis.mjs");
    lenis = new Lenis({ duration: 1.05, easing: (t) => 1 - Math.pow(1 - t, 3.2) });
    // The snap tool destroys lenis for deterministic screenshots; the raf
    // loop must actually stop, or it keeps pinning scroll to lenis's position.
    let rafRunning = true;
    window.__todokeLenisDestroy = () => { rafRunning = false; lenis.destroy(); };
    const raf = (time) => {
      if (!rafRunning) return;
      lenis.raf(time);
      requestAnimationFrame(raf);
    };
    requestAnimationFrame(raf);
  } catch (e) {
    // smooth scroll is enhancement, never a requirement
  }
}
initLenis();
reduceMotion.addEventListener("change", () => {
  if (reduceMotion.matches && lenis) { lenis.destroy(); lenis = null; }
});

// ---------------------------------------------------------------- reveals
// Content is visible by default; JS only adds the entrance when it can run.
const revealables = document.querySelectorAll(
  ".block, .stat, .proof, .hero__shot, .section__head, .term, .dl"
);

if (!reduceMotion.matches && "IntersectionObserver" in window) {
  const groups = new Map(); // parent -> index for 60ms stagger
  revealables.forEach((el) => {
    const key = el.parentElement;
    const i = groups.get(key) ?? 0;
    groups.set(key, i + 1);
    el.classList.add("reveal");
    el.style.setProperty("--reveal-delay", `${Math.min(i * 60, 240)}ms`);
  });

  const io = new IntersectionObserver(
    (entries) => {
      for (const entry of entries) {
        if (entry.isIntersecting) {
          entry.target.classList.add("is-in");
          io.unobserve(entry.target);
        }
      }
    },
    { rootMargin: "0px 0px -8% 0px", threshold: 0.08 }
  );
  revealables.forEach((el) => io.observe(el));
}

// ---------------------------------------------------------------- measured counters
// Count the stat numbers up when the section enters. Reduced motion: skip.
const stats = document.querySelectorAll(".stat__num[data-count]");

function renderCount(el, value, suffix) {
  el.textContent = value.toLocaleString("en-US") + suffix;
}

function animateCount(el) {
  const target = Number(el.dataset.count);
  const suffix = el.dataset.suffix ?? "";
  if (reduceMotion.matches || target === 0) {
    renderCount(el, target, suffix);
    return;
  }
  const t0 = performance.now();
  const dur = 1100;
  const tick = (now) => {
    const p = Math.min((now - t0) / dur, 1);
    const eased = 1 - Math.pow(1 - p, 4);
    renderCount(el, Math.round(target * eased), suffix);
    if (p < 1) requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
}

if (stats.length && "IntersectionObserver" in window) {
  const seen = new WeakSet();
  const counter = new IntersectionObserver(
    (entries) => {
      for (const entry of entries) {
        if (entry.isIntersecting && !seen.has(entry.target)) {
          seen.add(entry.target);
          animateCount(entry.target);
          counter.unobserve(entry.target);
        }
      }
    },
    { threshold: 0.5 }
  );
  stats.forEach((el) => counter.observe(el));
} else {
  stats.forEach((el) => renderCount(el, Number(el.dataset.count), el.dataset.suffix ?? ""));
}
