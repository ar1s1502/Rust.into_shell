import { defineConfig } from 'vite';
import tailwindcss from '@tailwindcss/vite'; // If using Tailwind v4

export default defineConfig({
  plugins: [
    tailwindcss(),
  ],
});
