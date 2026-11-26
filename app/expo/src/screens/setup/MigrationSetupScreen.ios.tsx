import { MigrationSetupContent } from "./MigrationSetupContent";
import { useSetupFlowController } from "./useSetupFlowController";

export function MigrationSetupScreen() {
  const ctrl = useSetupFlowController();
  return <MigrationSetupContent ctrl={ctrl} standaloneAlerts />;
}
