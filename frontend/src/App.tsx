import { AuthPage } from './AuthPage';
import { ProblemList } from './ProblemList';
import { useRoute } from './useRoute';
import { AuthProvider } from './useAuth';
import { Workspace } from './Workspace';

/**
 * `/s/<scenarioId>` opens that problem, `/login` is the account form, anything else is
 * the problem list. There is deliberately no redirect wall: browsing problems and drawing
 * on the canvas work signed out, and only saving and critiquing need an account.
 */
function Routes() {
  const { path, navigate } = useRoute();
  const scenarioId = path.startsWith('/s/') ? decodeURIComponent(path.slice(3)) : null;

  if (path === '/login') {
    return <AuthPage onDone={() => navigate('/')} onBack={() => navigate('/')} />;
  }

  return scenarioId ? (
    <Workspace scenarioId={scenarioId} onBack={() => navigate('/')} />
  ) : (
    <ProblemList
      onOpen={(id) => navigate(`/s/${encodeURIComponent(id)}`)}
      onSignIn={() => navigate('/login')}
    />
  );
}

export default function App() {
  return (
    <AuthProvider>
      <Routes />
    </AuthProvider>
  );
}
