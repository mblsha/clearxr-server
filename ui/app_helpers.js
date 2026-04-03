(function registerHelpers(globalObject, factory) {
  const helpers = factory();

  if (typeof module === "object" && module.exports) {
    module.exports = helpers;
  }

  globalObject.clearXrUiHelpers = helpers;
})(typeof globalThis !== "undefined" ? globalThis : this, () => {
  function formatHealthLabel(health) {
    switch (health) {
      case "running":
        return "Running";
      case "paused":
        return "Listening";
      case "stopped":
      default:
        return "Stopped";
    }
  }

  function normalizeAddressOption(entry) {
    if (typeof entry === "string") {
      return { value: entry, label: entry };
    }

    const address = entry?.address || "";
    const interfaceName = entry?.interfaceName || "";
    const label = interfaceName ? `${address} (${interfaceName})` : address;
    return { value: address, label };
  }

  function firstAddressValue(addresses, fallbackValue = "") {
    const firstEntry = addresses[0];
    if (typeof firstEntry === "string") {
      return firstEntry;
    }

    return firstEntry?.address || fallbackValue;
  }

  function isServerRunning(snapshot) {
    if (!snapshot) {
      return false;
    }

    return (
      snapshot.sessionManagement.health !== "stopped" ||
      snapshot.bonjour.health !== "stopped"
    );
  }

  function isOpenXrReady(status) {
    return Boolean(status?.runtimeIsActive && status?.layerIsRegistered);
  }

  function openXrActionLabel(status) {
    if (status?.runtimeIsActive && !status?.layerIsRegistered) {
      return "Register OpenXR Layer";
    }

    if (isOpenXrReady(status)) {
      return "Reapply OpenXR Registration";
    }

    return "Register OpenXR Runtime + Layer";
  }

  function openXrSummaryLabel(status) {
    if (isOpenXrReady(status)) {
      return "Registered";
    }

    return status?.runtimeIsActive ? "Layer Needed" : "Needs Registration";
  }

  function openXrActionNote(status) {
    if (status?.runtimeIsActive) {
      return "Clear XR is already selected as the OpenXR runtime. The button can still reapply the layer/runtime registration if needed.";
    }

    return "Runtime registration writes the machine-wide OpenXR ActiveRuntime key, so Windows may require administrator rights.";
  }

  return {
    firstAddressValue,
    formatHealthLabel,
    isOpenXrReady,
    isServerRunning,
    normalizeAddressOption,
    openXrActionLabel,
    openXrActionNote,
    openXrSummaryLabel,
  };
});
