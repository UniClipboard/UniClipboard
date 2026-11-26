import { useEffect, useState } from "react";
import { useEngineStore } from "@/engine/store";

export function useSendDevices(visible: boolean) {
  const devices = useEngineStore((state) => state.devices);
  const [selection, setSelection] = useState<string[]>([]);
  const trustedDevices = devices.filter((device) => device.trust === "trusted");
  const selectedDeviceIds = selection.filter((id) => trustedDevices.some((device) => device.id === id));

  useEffect(() => {
    if (!visible) setSelection([]);
  }, [visible]);

  const toggleDeviceSelection = (id: string) => {
    setSelection((current) => current.includes(id)
      ? current.filter((selected) => selected !== id)
      : [...current, id]);
  };

  return { trustedDevices, selectedDeviceIds, toggleDeviceSelection };
}
