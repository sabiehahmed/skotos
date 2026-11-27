import { API_CLIENT_NAME, API_CLIENT_VERSION } from "@skotos/api-client";
import { SkotosShellStub, UI_LIBRARY_NAME } from "@skotos/ui";

export function getAppMetadata() {
  return {
    appName: "@skotos/web",
    uiLibrary: UI_LIBRARY_NAME,
    apiClient: `${API_CLIENT_NAME}@${API_CLIENT_VERSION}`,
  };
}

export function App() {
  const meta = getAppMetadata();
  return (
    <SkotosShellStub
      title="Skotos Control Plane"
      subtitle={`${meta.appName} -> ${meta.uiLibrary} + ${meta.apiClient}`}
    />
  );
}

export default App;
