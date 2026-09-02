/// <reference types="vite/client" />

declare module '*.vue' {
  import type { DefineComponent } from 'vue';
  const component: DefineComponent<object, object, unknown>;
  export default component;
}

declare module '*.json' {
  const value: Record<string, unknown>;
  export default value;
}

declare module '*.png' {
  const src: string;
  export default src;
}

declare module '*.ico' {
  const src: string;
  export default src;
}
