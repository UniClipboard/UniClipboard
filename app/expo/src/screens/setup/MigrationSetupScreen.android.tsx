import { useEffect } from "react";
import { BackHandler } from "react-native";
import { MigrationSetupContent } from "./MigrationSetupContent";
import { useSetupFlowController } from "./useSetupFlowController";

export function MigrationSetupScreen() {
  const ctrl = useSetupFlowController();

  useEffect(() => {
    const sub = BackHandler.addEventListener("hardwareBackPress", () => {
      if (ctrl.setupViewState === "m-importing" || ctrl.setupViewState === "m-verifying") {
        ctrl.requestStop();
        return true;
      }
      if (ctrl.setupViewState === "m-stopping") {
        return true;
      }
      return false;
    });
    return () => sub.remove();
  }, [ctrl]);

  return <MigrationSetupContent ctrl={ctrl} />;
}
