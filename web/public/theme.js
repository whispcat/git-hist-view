// Applies a saved theme before first paint, so dark-mode users never see a light flash.
try {
  const t = localStorage.getItem('ghv:theme');
  if (t === 'light' || t === 'dark') document.documentElement.dataset.theme = t;
} catch {}
