import { useCallback, useEffect, useState } from 'react';

/**
 * Two routes do not justify a router dependency. `pushState` plus a `popstate` listener
 * is the whole feature, and it keeps back/forward and shareable URLs working.
 *
 * Vite's dev server already falls back to index.html for unknown paths, so /s/<id> loads
 * on a cold request with no extra config. Serving the built bundle from anything other
 * than the dev server needs that same fallback — the same caveat as the missing CORS layer.
 */
export function useRoute() {
  const [path, setPath] = useState(() => window.location.pathname);

  useEffect(() => {
    const onPop = () => setPath(window.location.pathname);
    window.addEventListener('popstate', onPop);
    return () => window.removeEventListener('popstate', onPop);
  }, []);

  const navigate = useCallback((to: string) => {
    window.history.pushState(null, '', to);
    setPath(to);
  }, []);

  return { path, navigate };
}
