import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
// Sets data-theme on <html> before the first paint.
import "./state/theme";
import "./styles.css";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
