import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { applyTheme, loadThemePreference } from "./lib/theme";
import "./global.css";

// Apply the stored preference before the first paint so the shell never
// flashes the wrong theme while React mounts.
applyTheme(loadThemePreference());

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
