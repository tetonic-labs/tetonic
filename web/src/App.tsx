import { LocalEngineProvider } from './context/LocalEngineContext';
import { TeamWorkspace } from './components/team-work/TeamWorkspace';

// One product surface. Old URLs resolve here; there is no preview UI switch.
export function App() {
  return (
    <LocalEngineProvider>
      <TeamWorkspace />
    </LocalEngineProvider>
  );
}
