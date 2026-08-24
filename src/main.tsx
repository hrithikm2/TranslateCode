const pathname = window.location.pathname.replace(/\/+$/, '') || '/';

const routes: Record<string, { title: string; description: string; load: () => Promise<unknown> }> = {
  '/translate': {
    title: 'Translate Code — TranslateCode',
    description: 'Translate code between seven programming languages locally in your browser.',
    load: () => import('./features/translate-code/main.jsx'),
  },
  '/translate-code': {
    title: 'Translate Code — TranslateCode',
    description: 'Translate code between seven programming languages locally in your browser.',
    load: () => import('./features/translate-code/main.jsx'),
  },
  '/visualise': {
    title: 'Visualise Code — TranslateCode',
    description: 'Run Python locally and step through algorithms line by line.',
    load: () => import('./features/code-visualiser/main.tsx'),
  },
  '/visualize': {
    title: 'Visualise Code — TranslateCode',
    description: 'Run Python locally and step through algorithms line by line.',
    load: () => import('./features/code-visualiser/main.tsx'),
  },
  '/code-visualiser': {
    title: 'Visualise Code — TranslateCode',
    description: 'Run Python locally and step through algorithms line by line.',
    load: () => import('./features/code-visualiser/main.tsx'),
  },
};

const route = routes[pathname] ?? {
  title: 'TranslateCode — Choose your workspace',
  description: 'Translate code across languages or visualise Python execution, entirely in your browser.',
  load: () => import('./features/home/main.tsx'),
};

document.title = route.title;
document.querySelector('meta[name="description"]')?.setAttribute('content', route.description);

route.load().catch((error) => {
  console.error('Unable to load the selected workspace.', error);
  const root = document.getElementById('root');
  if (root) root.textContent = 'This workspace could not be loaded. Please refresh and try again.';
});
