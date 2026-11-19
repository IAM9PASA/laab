/** Windows has Ctrl where macOS has ⌘, and a native title bar. */
export const isWindows = typeof navigator !== "undefined" && navigator.userAgent.includes("Windows");
