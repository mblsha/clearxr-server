const test = require("node:test");
const assert = require("node:assert/strict");

const {
  firstAddressValue,
  formatHealthLabel,
  isOpenXrReady,
  isServerRunning,
  normalizeAddressOption,
  openXrActionLabel,
  openXrActionNote,
  openXrSummaryLabel,
} = require("./app_helpers.js");

test("formatHealthLabel maps runtime health states", () => {
  assert.equal(formatHealthLabel("running"), "Running");
  assert.equal(formatHealthLabel("paused"), "Listening");
  assert.equal(formatHealthLabel("stopped"), "Stopped");
  assert.equal(formatHealthLabel("unknown"), "Stopped");
});

test("normalizeAddressOption keeps interface names in the label", () => {
  assert.deepEqual(normalizeAddressOption("192.168.1.9"), {
    value: "192.168.1.9",
    label: "192.168.1.9",
  });

  assert.deepEqual(
    normalizeAddressOption({
      address: "10.0.0.15",
      interfaceName: "Ethernet",
    }),
    {
      value: "10.0.0.15",
      label: "10.0.0.15 (Ethernet)",
    },
  );
});

test("firstAddressValue supports both string and object payloads", () => {
  assert.equal(firstAddressValue(["192.168.1.9"], "127.0.0.1"), "192.168.1.9");
  assert.equal(
    firstAddressValue([{ address: "10.0.0.15", interfaceName: "Ethernet" }]),
    "10.0.0.15",
  );
  assert.equal(firstAddressValue([], "127.0.0.1"), "127.0.0.1");
});

test("isServerRunning reflects either active session management or bonjour", () => {
  assert.equal(isServerRunning(null), false);
  assert.equal(
    isServerRunning({
      bonjour: { health: "stopped" },
      sessionManagement: { health: "stopped" },
    }),
    false,
  );
  assert.equal(
    isServerRunning({
      bonjour: { health: "running" },
      sessionManagement: { health: "stopped" },
    }),
    true,
  );
  assert.equal(
    isServerRunning({
      bonjour: { health: "stopped" },
      sessionManagement: { health: "paused" },
    }),
    true,
  );
});

test("OpenXR helpers keep the action text in sync with registration state", () => {
  const needsEverything = {
    runtimeIsActive: false,
    layerIsRegistered: false,
  };
  const needsLayer = {
    runtimeIsActive: true,
    layerIsRegistered: false,
  };
  const fullyReady = {
    runtimeIsActive: true,
    layerIsRegistered: true,
  };

  assert.equal(isOpenXrReady(needsEverything), false);
  assert.equal(isOpenXrReady(fullyReady), true);

  assert.equal(openXrActionLabel(needsEverything), "Register OpenXR Runtime + Layer");
  assert.equal(openXrActionLabel(needsLayer), "Register OpenXR Layer");
  assert.equal(openXrActionLabel(fullyReady), "Reapply OpenXR Registration");

  assert.equal(openXrSummaryLabel(needsEverything), "Needs Registration");
  assert.equal(openXrSummaryLabel(needsLayer), "Layer Needed");
  assert.equal(openXrSummaryLabel(fullyReady), "Registered");

  assert.match(openXrActionNote(needsEverything), /administrator rights/i);
  assert.match(openXrActionNote(fullyReady), /already selected as the OpenXR runtime/i);
});
