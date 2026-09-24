/// <reference types="vite/client" />

export {};

declare global {
  function showDirectoryPicker(options?: { mode?: 'read' | 'readwrite' }): Promise<FileSystemDirectoryHandle>;

  interface FileSystemDirectoryHandle {
    keys(): AsyncIterableIterator<string>;
  }
}

declare module 'solid-js' {
  namespace JSX {
    interface InputHTMLAttributes<T> {
      webkitdirectory?: boolean;
    }
  }
}
