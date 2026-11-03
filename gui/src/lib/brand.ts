// Brand constants for the UI. The name itself comes from `productName` in src-tauri/tauri.conf.json
// (injected by vite.config.ts); never write the product name as a literal anywhere else.
import markForDark from "../../assets/brand/cawget-mark-dark.svg";
import markForLight from "../../assets/brand/cawget-mark.svg";

export const PRODUCT_NAME: string = __PRODUCT_NAME__;
export const APP_VERSION: string = __APP_VERSION__;
/** localStorage prefix for UI preferences. */
export const PREFS_KEY = "cawget.ui";
/** Logo variant drawn for a dark (`dark`) or light (`light`) background. */
export const logoFor = (theme: "dark" | "light"): string => (theme === "dark" ? markForDark : markForLight);
