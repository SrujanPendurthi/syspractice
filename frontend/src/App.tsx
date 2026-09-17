import { AppBar } from './AppBar';
import { AuthPage } from './AuthPage';
import { Discuss } from './Discuss';
import { MatchRoom } from './MatchRoom';
import { Learn } from './Learn';
import { ProblemList } from './ProblemList';
import { useRoute } from './useRoute';
import { AuthProvider } from './useAuth';
import { SettingsProvider } from './useTheme';
import { Versus } from './Versus';
import { Workspace } from './Workspace';

/**
 * `/s/<scenarioId>` opens that problem, `/learn` is the concept index and
 * `/learn/<conceptId>` one article, `/discuss` and `/discuss/<postId>` are the forum,
 * `/versus` is the match lobby and `/m/<matchId>` one race, `/login` is the account form,
 * anything else is the problem list. There is deliberately no redirect wall: browsing
 * problems, reading concepts and drawing on the canvas all work signed out, and only
 * saving, critiquing and tracking what you have read need an account.
 */
function Routes() {
  const { path, navigate } = useRoute();
  const scenarioId = path.startsWith('/s/') ? decodeURIComponent(path.slice(3)) : null;
  const openScenario = (id: string) => navigate(`/s/${encodeURIComponent(id)}`);

  const page = (() => {
    if (path === '/login') {
      return <AuthPage onDone={() => navigate('/')} />;
    }

    if (path === '/learn' || path.startsWith('/learn/')) {
      const conceptId = path.startsWith('/learn/') ? decodeURIComponent(path.slice(7)) : null;
      return (
        <Learn
          conceptId={conceptId}
          // An empty id is the index; anything else is that article.
          onOpen={(id) => navigate(id ? `/learn/${encodeURIComponent(id)}` : '/learn')}
          onOpenScenario={openScenario}
          onSignIn={() => navigate('/login')}
        />
      );
    }

    if (path === '/discuss' || path.startsWith('/discuss/')) {
      // Non-numeric ids cannot exist, so anything unparseable is the index rather than a
      // request that would 404 on the way to the same place.
      const postId = Number(path.slice(9));
      return (
        <Discuss
          postId={Number.isInteger(postId) && postId > 0 ? postId : null}
          onOpen={(id) => navigate(id === null ? '/discuss' : `/discuss/${id}`)}
          onOpenScenario={openScenario}
          onSignIn={() => navigate('/login')}
        />
      );
    }

    if (path === '/versus') {
      return <Versus onOpen={(id) => navigate(`/m/${id}`)} onSignIn={() => navigate('/login')} />;
    }

    if (path.startsWith('/m/')) {
      const matchId = Number(path.slice(3));
      if (Number.isInteger(matchId) && matchId > 0) {
        return <MatchRoom matchId={matchId} onBack={() => navigate('/versus')} />;
      }
    }

    return scenarioId ? (
      <Workspace scenarioId={scenarioId} onBack={() => navigate('/')} />
    ) : (
      <ProblemList onOpen={openScenario} onSignIn={() => navigate('/login')} />
    );
  })();

  // The workspace and the match room lay out three panes that scroll independently, so
  // the shell must not wrap them in a second scrollbar.
  const fullBleed = !!scenarioId || path.startsWith('/m/');

  return (
    <div className="flex h-full flex-col bg-page">
      {/* Rendered once, here, rather than by each page. A page cannot restructure the
          bar — the most it can do is drop the section links, which is what the
          workspace does. */}
      <AppBar path={path} navigate={navigate} sections={!fullBleed} />
      <main className={`min-h-0 flex-1 ${fullBleed ? 'overflow-hidden' : 'overflow-y-auto'}`}>
        {page}
      </main>
    </div>
  );
}

export default function App() {
  return (
    <SettingsProvider>
      <AuthProvider>
        <Routes />
      </AuthProvider>
    </SettingsProvider>
  );
}
