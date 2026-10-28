import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "@fontsource/barlow/400.css";
import "@fontsource/barlow/500.css";
import "@fontsource/barlow/600.css";
import "@fontsource/barlow/700.css";
import "@fontsource/barlow-semi-condensed/500.css";
import "@fontsource/barlow-semi-condensed/600.css";
import "@fontsource/barlow-semi-condensed/700.css";
import "uplot/dist/uPlot.min.css";
import "./styles/app.css";
import { App } from "./App";
import { Overlay } from "./modules/overlay/Overlay";

// The gamer overlay is a second window of the same app.
const overlay = window.location.hash === "#overlay";

createRoot(document.getElementById("root")!).render(<StrictMode>{overlay ? <Overlay /> : <App />}</StrictMode>);
