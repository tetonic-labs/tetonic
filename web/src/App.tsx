import { LocalEngineProvider } from './context/LocalEngineContext';
import { TeamWorkspace } from './components/team-work/TeamWorkspace';
import { StartupGate } from './components/preload/StartupGate';

// One product surface. Old URLs resolve here; there is no preview UI switch.
export function App() {
  return (
    <LocalEngineProvider>
      <StartupGate>
        <TeamWorkspace />
      </StartupGate>
    </LocalEngineProvider>
  );
}
