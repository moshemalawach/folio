(() => {
  const steps = document.querySelectorAll(".step");
  const screens = document.querySelectorAll(".stage__screen");
  const caption = document.querySelector("[data-caption]");
  const index = document.querySelector(".stage__index");
  const captions = [
    "A note container on hover, with its paragraph handle and tags.",
    "Ink, shape recognition and equations on one page.",
    "A PDF printout becomes a page you can write on.",
    "Immersive Reader with line focus and the system voice.",
  ];

  const show = (active) => {
    steps.forEach((step, i) => step.classList.toggle("is-active", i === active));
    screens.forEach((screen, i) => screen.classList.toggle("is-active", i === active));
    if (caption) caption.textContent = captions[active] || "";
    if (index) index.textContent = String(active + 1).padStart(2, "0") + " / 04";
  };

  if ("IntersectionObserver" in window) {
    const observer = new IntersectionObserver(
      (entries) => {
        const visible = entries
          .filter((entry) => entry.isIntersecting)
          .sort((a, b) => b.intersectionRatio - a.intersectionRatio)[0];
        if (visible) show(Number(visible.target.dataset.step));
      },
      { rootMargin: "-36% 0px -48% 0px", threshold: [0, 0.25, 0.5, 0.75, 1] },
    );
    steps.forEach((step) => observer.observe(step));
  }

  const buttons = document.querySelectorAll("[data-theme]");
  const images = document.querySelectorAll("[data-theme-image]");

  const setTheme = (theme) => {
    buttons.forEach((button) => {
      const active = button.dataset.theme === theme;
      button.classList.toggle("is-active", active);
      button.setAttribute("aria-pressed", String(active));
    });
    images.forEach((image) => image.classList.toggle("is-active", image.dataset.themeImage === theme));
  };

  buttons.forEach((button) => {
    button.addEventListener("click", () => setTheme(button.dataset.theme));
  });
})();
