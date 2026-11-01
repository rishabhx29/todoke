/* Todoke hero — a stream of packets arriving down a lane grid.
   Three.js is dynamically imported only when the hero is on screen,
   WebGL exists, and the visitor hasn't asked for reduced motion.
   Reduced-motion fallback: the static CSS atmosphere + streaks (they
   collapse to a still frame under the global reduce rule) — no WebGL
   download at all. */

const canvas = document.getElementById("hero-canvas");

function webglOK() {
  try {
    const probe = document.createElement("canvas");
    const gl = probe.getContext("webgl2") || probe.getContext("webgl");
    if (!gl) return false;
    const lose = gl.getExtension("WEBGL_lose_context");
    if (lose) lose.loseContext();
    return true;
  } catch {
    return false;
  }
}

function shouldRun() {
  if (!canvas) return false;
  if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return false;
  return webglOK();
}

if (shouldRun()) {
  let started = false;
  let disposeScene = null;

  const start = async () => {
    if (started) return;
    started = true;

    let THREE;
    try {
      THREE = await import("../vendor/three/three.module.min.js");
    } catch (e) {
      console.warn("hero: three.js failed to load, keeping CSS fallback", e);
      return;
    }

    try {
      const LINE = 0x2b3138;
      const DIM = 0x9fa5ac;
      const TEAL = 0x16827d;
      const VOLT = 0xf2e309;

      const renderer = new THREE.WebGLRenderer({
        canvas,
        alpha: true,
        antialias: true,
        powerPreference: "low-power",
      });
      renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 1.75));

      const scene = new THREE.Scene();
      scene.fog = new THREE.FogExp2(0x080d11, 0.045);

      const camera = new THREE.PerspectiveCamera(58, 1, 0.1, 120);
      camera.position.set(0, 0.35, 7);

      /* ---- lane grid: longitudinal lines on floor + ceiling ---- */
      const laneSpan = { from: -60, to: 6 };
      const laneGroup = new THREE.Group();
      const laneMat = new THREE.LineBasicMaterial({
        color: LINE, transparent: true, opacity: 0.85, fog: true,
      });
      const lanePts = [];
      for (const x of [-7.5, -5.5, -3.5, -1.5, 1.5, 3.5, 5.5, 7.5]) {
        lanePts.push(x, -2.9, laneSpan.from, x, -2.9, laneSpan.to);
        lanePts.push(x, 3.1, laneSpan.from, x, 3.1, laneSpan.to);
      }
      // the one volt lane, center-right so it reads under the CTA side
      const voltMat = new THREE.LineBasicMaterial({
        color: VOLT, transparent: true, opacity: 0.5, fog: true,
      });
      const laneGeo = new THREE.BufferGeometry();
      laneGeo.setAttribute("position", new THREE.Float32BufferAttribute(lanePts, 3));
      laneGroup.add(new THREE.LineSegments(laneGeo, laneMat));
      const voltLaneGeo = new THREE.BufferGeometry();
      voltLaneGeo.setAttribute("position",
        new THREE.Float32BufferAttribute([0.6, -2.9, laneSpan.from, 0.6, -2.9, laneSpan.to], 3));
      laneGroup.add(new THREE.LineSegments(voltLaneGeo, voltMat));

      /* ---- cross ties: two groups leapfrogging for a seamless run ---- */
      const tieSpacing = 4;
      const tieLen = 40; // z-length covered by one group
      const tiesMat = new THREE.LineBasicMaterial({
        color: LINE, transparent: true, opacity: 0.55, fog: true,
      });
      const tiePts = [];
      for (let z = 0; z > -tieLen; z -= tieSpacing) {
        tiePts.push(-7.5, -2.9, z, 7.5, -2.9, z);
      }
      const tieGeo = new THREE.BufferGeometry();
      tieGeo.setAttribute("position", new THREE.Float32BufferAttribute(tiePts, 3));
      const tiesA = new THREE.LineSegments(tieGeo, tiesMat);
      const tiesB = new THREE.LineSegments(tieGeo, tiesMat);
      tiesA.position.z = 0;
      tiesB.position.z = -tieLen;
      laneGroup.add(tiesA, tiesB);
      scene.add(laneGroup);

      /* ---- horizon glow: two soft gradient planes far down the tunnel ---- */
      const glowTexture = (r, g, b) => {
        const c = document.createElement("canvas");
        c.width = c.height = 128;
        const ctx = c.getContext("2d");
        const grad = ctx.createRadialGradient(64, 64, 4, 64, 64, 64);
        grad.addColorStop(0, `rgba(${r},${g},${b},0.55)`);
        grad.addColorStop(1, `rgba(${r},${g},${b},0)`);
        ctx.fillStyle = grad;
        ctx.fillRect(0, 0, 128, 128);
        const tex = new THREE.CanvasTexture(c);
        tex.colorSpace = THREE.SRGBColorSpace;
        return tex;
      };
      const glowVolt = new THREE.Mesh(
        new THREE.PlaneGeometry(18, 9),
        new THREE.MeshBasicMaterial({
          map: glowTexture(242, 227, 9), transparent: true, opacity: 0.34,
          blending: THREE.AdditiveBlending, depthWrite: false, fog: false,
        })
      );
      glowVolt.position.set(0.6, -1.6, -54);
      const glowTeal = new THREE.Mesh(
        new THREE.PlaneGeometry(30, 16),
        new THREE.MeshBasicMaterial({
          map: glowTexture(22, 130, 125), transparent: true, opacity: 0.34,
          blending: THREE.AdditiveBlending, depthWrite: false, fog: false,
        })
      );
      glowTeal.position.set(-9, 2.2, -50);
      scene.add(glowVolt, glowTeal);

      /* ---- packets: instanced streaks flying toward the viewer ---- */
      const COUNT = 90;
      const packetGeo = new THREE.BoxGeometry(0.055, 0.055, 0.62);
      const packetMat = new THREE.MeshBasicMaterial({
        toneMapped: false, blending: THREE.AdditiveBlending, depthWrite: false, fog: true,
      });
      const packets = new THREE.InstancedMesh(packetGeo, packetMat, COUNT);
      const dummy = new THREE.Object3D();
      const laneXs = [-7, -5, -3, -1, 1, 3, 5, 7];
      const state = [];
      const color = new THREE.Color();
      for (let i = 0; i < COUNT; i++) {
        const floor = Math.random() < 0.82;
        const roll = Math.random();
        state.push({
          x: laneXs[(Math.random() * laneXs.length) | 0] + (Math.random() - 0.5) * 0.5,
          y: floor ? -2.9 + 0.05 + Math.random() * 0.25 : 3.1 - 0.05 - Math.random() * 0.25,
          z: laneSpan.from + Math.random() * (laneSpan.to - laneSpan.from),
          speed: 9 + Math.random() * 14,
        });
        if (roll < 0.08) color.setHex(VOLT).multiplyScalar(1.15);
        else if (roll < 0.24) color.setHex(TEAL).multiplyScalar(1.1);
        else color.setHex(DIM).multiplyScalar(0.45 + Math.random() * 0.35);
        packets.setColorAt(i, color);
      }
      packets.instanceColor.needsUpdate = true;
      scene.add(packets);

      /* ---- interaction state ---- */
      const mouse = { x: 0, y: 0, tx: 0, ty: 0 };
      const fine = window.matchMedia("(hover: hover) and (pointer: fine)");
      if (fine.matches) {
        window.addEventListener("pointermove", (e) => {
          mouse.tx = (e.clientX / window.innerWidth) * 2 - 1;
          mouse.ty = (e.clientY / window.innerHeight) * 2 - 1;
        }, { passive: true });
      }

      const resize = () => {
        const w = canvas.clientWidth || canvas.parentElement.clientWidth;
        const h = canvas.clientHeight || canvas.parentElement.clientHeight;
        renderer.setSize(w, h, false);
        camera.aspect = w / h;
        camera.updateProjectionMatrix();
      };
      resize();
      const ro = new ResizeObserver(resize);
      ro.observe(canvas);

      let heroVisible = true;
      let rafId = 0;
      let running = false;
      const tick = (t) => {
        rafId = 0;
        if (!running) return;
        const time = t * 0.001;

        // camera: mouse parallax + gentle dolly as the hero scrolls away
        mouse.x += (mouse.tx - mouse.x) * 0.055;
        mouse.y += (mouse.ty - mouse.y) * 0.055;
        const heroH = canvas.parentElement.offsetHeight || 1;
        const scrollT = Math.min(Math.max(window.scrollY / heroH, 0), 1);
        camera.position.x = mouse.x * 0.8;
        camera.position.y = 0.35 - mouse.y * 0.45;
        camera.position.z = 7 + scrollT * 2.2;
        camera.lookAt(mouse.x * 0.4, -mouse.y * 0.2, -18);

        // ties run toward the camera, seamless via leapfrog
        const tz = (time * 9) % tieLen;
        tiesA.position.z = tz;
        tiesB.position.z = tz - tieLen;

        for (let i = 0; i < COUNT; i++) {
          const s = state[i];
          s.z += s.speed * (1 / 60);
          if (s.z > laneSpan.to + 2) {
            s.z = laneSpan.from - Math.random() * 8;
            s.speed = 9 + Math.random() * 14;
          }
          dummy.position.set(s.x, s.y, s.z);
          const stretch = 1 + s.speed / 14;
          dummy.scale.set(1, 1, stretch);
          dummy.updateMatrix();
          packets.setMatrixAt(i, dummy.matrix);
        }
        packets.instanceMatrix.needsUpdate = true;

        renderer.render(scene, camera);
        rafId = requestAnimationFrame(tick);
      };
      const play = () => {
        if (running || rafId) return;
        running = true;
        rafId = requestAnimationFrame(tick);
      };
      const pause = () => {
        running = false;
        if (rafId) { cancelAnimationFrame(rafId); rafId = 0; }
      };

      document.addEventListener("visibilitychange", () => {
        document.hidden ? pause() : (heroVisible && play());
      });
      window.addEventListener("pagehide", () => { pause(); dispose(); }, { once: true });

      function dispose() {
        ro.disconnect();
        visibilityIo.disconnect();
        laneGeo.dispose();
        voltLaneGeo.dispose();
        tieGeo.dispose();
        packetGeo.dispose();
        packetMat.dispose();
        laneMat.dispose();
        tiesMat.dispose();
        voltMat.dispose();
        glowVolt.geometry.dispose();
        glowVolt.material.dispose();
        glowVolt.material.map?.dispose();
        glowTeal.geometry.dispose();
        glowTeal.material.dispose();
        glowTeal.material.map?.dispose();
        renderer.dispose();
        renderer.forceContextLoss?.();
      }

      // first frame, then announce + keep the loop alive only while visible
      renderer.render(scene, camera);
      document.documentElement.classList.add("has-3d");
      play();
      const visibilityIo = new IntersectionObserver((entries) => {
        heroVisible = entries[0].isIntersecting;
        entries[0].isIntersecting ? play() : pause();
      }, { threshold: 0.02 });
      visibilityIo.observe(canvas);
    } catch (e) {
      console.warn("hero: WebGL scene failed, keeping CSS fallback", e);
    }
  };

  // lazy gate: don't even download three.js until the hero is on screen
  const gate = new IntersectionObserver((entries) => {
    if (entries[0].isIntersecting) {
      gate.disconnect();
      start();
    }
  }, { rootMargin: "200px" });
  gate.observe(canvas);
}
