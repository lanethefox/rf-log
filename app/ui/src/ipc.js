// Transport abstraction: the UI talks to the backend only through `api`, so a
// future web build can swap Tauri invoke/listen for HTTP/WS without touching
// components (docs/RF-LOG-v2.md §4.7).
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export const api = {
  createMission: (name, bands) => invoke("create_mission", { name, bands }),
  startMission: (id) => invoke("start_mission", { id }),
  stopMission: () => invoke("stop_mission"),
  listMissions: () => invoke("list_missions"),
  listDetections: (id, limit = 200) => invoke("list_detections", { id, limit }),
  getStatus: () => invoke("get_status"),
  listDevices: () => invoke("list_devices"),
  refreshDevices: () => invoke("refresh_devices"),
  // cfg keys are camelCase; Tauri maps them to the snake_case command params.
  setDeviceConfig: (id, cfg) => invoke("set_device_config", { id, ...cfg }),
  // returns a Promise<UnlistenFn>
  on: (event, cb) => listen(event, (e) => cb(e.payload)),
};
