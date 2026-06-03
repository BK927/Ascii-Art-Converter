import { defineConfig } from "vite";

export default defineConfig({
  base: "/Ascii-Art-Converter/",
  build: {
    target: "es2022",
  },
  worker: {
    format: "es",
  },
});
