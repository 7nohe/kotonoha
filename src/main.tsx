import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import Overlay from "./windows/overlay/Overlay";
import Settings from "./windows/settings/Settings";
import "./global.css";

const label = getCurrentWindow().label;

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    {label === "settings" ? <Settings /> : <Overlay />}
  </React.StrictMode>,
);
