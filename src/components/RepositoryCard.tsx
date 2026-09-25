import { useMemo, useState } from "react";
import type { RepositoryView } from "../bindings/RepositoryView";
import { errorMessage } from "../ipc/backend";
import { useBackend } from "../ipc/BackendContext";
import { formatTime } from "../state/format";
import { aheadBehind, branchLabel, describeChange, folderName } from "../state/git";
import { useOfficeStore } from "../state/store";

const FILES_SHOWN = 40;

/**
 * One working tree: branch, upstream, changed files, recent commits and
 * worktrees. Agents are named next to a file or commit only with evidence
 * (their tool wrote the file / they ran the `git commit`).
 */
export function RepositoryCard({ repo, title }: { repo: RepositoryView; title?: string }) {
  const backend = useBackend();
  const sessions = useOfficeStore((s) => s.sessions);
  const agents = useOfficeStore((s) => s.agents);
  const select = useOfficeStore((s) => s.select);
  const pushToast = useOfficeStore((s) => s.pushToast);
  const [allFiles, setAllFiles] = useState(false);
  const root = repo.location.worktreeRoot;
  const snap = repo.snapshot;
  const status = snap?.status;

  const agentName = (sessionKey: string) => {
    const s = sessions[sessionKey];
    return (s && agents[s.mainAgentKey]?.name) ?? s?.title ?? sessionKey.split(":").pop() ?? sessionKey;
  };
  const openAgent = (sessionKey: string) => {
    const s = sessions[sessionKey];
    if (s) select(s.mainAgentKey);
  };
  const writers = useMemo(() => new Map(repo.fileSessions.map((f) => [f.path, f.sessions])), [repo.fileSessions]);
  const committers = useMemo(() => new Map(repo.commitSessions.map((c) => [c.sha, c.session])), [repo.commitSessions]);

  const act = async (label: string, action: () => Promise<void>) => {
    try {
      await action();
    } catch (error) {
      pushToast("error", `${label}: ${errorMessage(error)}`);
    }
  };

  const files = status?.files ?? [];
  const shown = allFiles ? files : files.slice(0, FILES_SHOWN);
  const ab = status ? aheadBehind(status) : null;

  return (
    <section className="card repo-card">
      <div className="card-title-row">
        <h3>
          {title ?? folderName(repo.location.repositoryRoot)}
          {repo.location.linkedWorktree && <span className="muted"> · worktree {folderName(root)}</span>}
        </h3>
        <button className="btn small" onClick={() => act("Open folder", () => backend.openFolder(root))}>
          Open folder
        </button>
      </div>
      <p className="mono small muted">{root}</p>
      {repo.error && <p className="bad-text small">{repo.error}</p>}
      {status && (
        <p>
          <span className="chip">⎇ {branchLabel(status)}</span>{" "}
          {status.upstream ? (
            <span className="small">
              {ab} <span className="muted">vs {status.upstream}</span>
            </span>
          ) : (
            <span className="small muted">no upstream branch</span>
          )}
          {repo.refreshedAt && <span className="small muted"> · read {formatTime(repo.refreshedAt)}</span>}
        </p>
      )}
      {repo.sessions.length > 0 && (
        <p className="small">
          Agents here:{" "}
          {repo.sessions.map((key, i) => (
            <span key={key}>
              {i > 0 && ", "}
              <button className="link" onClick={() => openAgent(key)}>
                {agentName(key)}
              </button>
              {sessions[key]?.status === "ended" && <span className="muted"> (ended)</span>}
            </span>
          ))}
        </p>
      )}

      {status && (
        <>
          <h4>
            Changes{" "}
            <span className="muted small">
              {status.counts.unstaged} changed · {status.counts.untracked} new · {status.counts.staged} staged
              {status.counts.conflicted > 0 && ` · ${status.counts.conflicted} in conflict`}
            </span>
          </h4>
          {files.length === 0 ? (
            <p className="muted small">Nothing to commit.</p>
          ) : (
            <table className="table compact">
              <tbody>
                {shown.map((f) => {
                  const change = describeChange(f);
                  const by = writers.get(f.path) ?? [];
                  return (
                    <tr key={f.path}>
                      <td className="mono git-code" title={change.text}>
                        {change.code}
                      </td>
                      <td className="mono small" title={f.origPath ? `from ${f.origPath}` : undefined}>
                        {f.path}
                      </td>
                      <td className="small">
                        {by.length > 0 ? (
                          <>
                            written by{" "}
                            {by.map((k, i) => (
                              <span key={k}>
                                {i > 0 && ", "}
                                <button className="link" onClick={() => openAgent(k)}>
                                  {agentName(k)}
                                </button>
                              </span>
                            ))}
                          </>
                        ) : (
                          <span className="muted" title="No agent tool is known to have written this file">
                            not linked to an agent
                          </span>
                        )}
                      </td>
                      <td>
                        <button
                          className="btn small"
                          title="Show the file in its folder (it is not opened or run)"
                          onClick={() => act("Show file", () => backend.revealFile(root, f.path))}
                        >
                          Show
                        </button>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          )}
          {!allFiles && files.length > FILES_SHOWN && (
            <button className="link small" onClick={() => setAllFiles(true)}>
              Show all {files.length} files
            </button>
          )}
          {status.filesTruncated && (
            <p className="muted small">Only the first {files.length} changed files are listed.</p>
          )}
        </>
      )}

      {snap && snap.recentCommits.length > 0 && (
        <>
          <h4>Recent commits</h4>
          <table className="table compact">
            <tbody>
              {snap.recentCommits.map((c) => {
                const by = committers.get(c.sha);
                return (
                  <tr key={c.sha}>
                    <td className="mono small">{c.shortSha}</td>
                    <td className="small">{c.summary}</td>
                    <td className="small muted">{c.author}</td>
                    <td className="small muted">{new Date(c.time).toLocaleString()}</td>
                    <td className="small">
                      {by && (
                        <>
                          by{" "}
                          <button className="link" onClick={() => openAgent(by)}>
                            {agentName(by)}
                          </button>
                        </>
                      )}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </>
      )}

      {snap && snap.worktrees.length > 1 && (
        <>
          <h4>Worktrees</h4>
          <ul className="plain-list small">
            {snap.worktrees.map((w) => (
              <li key={w.path}>
                <span className="mono">{w.path}</span> · {w.branch ?? (w.detached ? "detached" : w.bare ? "bare" : "—")}
                {w.locked && " · locked"}
                {w.prunable && " · missing"}
              </li>
            ))}
          </ul>
        </>
      )}
    </section>
  );
}
