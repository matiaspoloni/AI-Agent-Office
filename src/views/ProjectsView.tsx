import { useState } from "react";
import { errorMessage } from "../ipc/backend";
import { useBackend } from "../ipc/BackendContext";
import { useOfficeStore } from "../state/store";

export function ProjectsView() {
  const backend = useBackend();
  const projects = useOfficeStore((s) => s.projects);
  const sessions = useOfficeStore((s) => s.sessions);
  const setProjects = useOfficeStore((s) => s.setProjects);
  const pushToast = useOfficeStore((s) => s.pushToast);
  const [name, setName] = useState("");
  const [path, setPath] = useState("");
  const [model, setModel] = useState("");

  const refresh = async () => setProjects(await backend.listProjects());

  const add = async () => {
    try {
      await backend.addProject({ name, path, defaultModel: model.trim() || undefined });
      setName("");
      setPath("");
      setModel("");
      await refresh();
    } catch (error) {
      pushToast("error", errorMessage(error));
    }
  };

  const remove = async (id: string) => {
    try {
      await backend.removeProject(id);
      await refresh();
    } catch (error) {
      pushToast("error", errorMessage(error));
    }
  };

  const browse = async () => {
    const folder = await backend.pickFolder();
    if (folder) {
      setPath(folder);
      if (!name.trim()) setName(folder.split(/[\\/]/).filter(Boolean).pop() ?? "");
    }
  };

  return (
    <div className="projects">
      <section className="card">
        <h3>Add a project</h3>
        <p className="muted small">
          Sessions whose working folder is inside a project are grouped under it automatically.
        </p>
        <div className="grid-3">
          <label>
            Name
            <input value={name} onChange={(e) => setName(e.target.value)} placeholder="Nalu" />
          </label>
          <label>
            Folder
            <div className="input-row">
              <input value={path} onChange={(e) => setPath(e.target.value)} placeholder="C:\\Projects\\Nalu" />
              {backend.kind === "desktop" && (
                <button className="btn" onClick={browse}>
                  Browse
                </button>
              )}
            </div>
          </label>
          <label>
            Default model <small className="muted">(optional)</small>
            <input value={model} onChange={(e) => setModel(e.target.value)} placeholder="provider default" />
          </label>
        </div>
        <div className="button-row end">
          <button className="btn primary" disabled={!name.trim() || !path.trim()} onClick={add}>
            Add project
          </button>
        </div>
      </section>

      <section className="card">
        <h3>Projects</h3>
        {projects.length === 0 && <p className="muted">No projects yet.</p>}
        <table className="table">
          <thead>
            <tr>
              <th>Name</th>
              <th>Folder</th>
              <th>Default model</th>
              <th>Active sessions</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {projects.map((p) => {
              const active = Object.values(sessions).filter((s) => s.projectId === p.id && s.status === "active").length;
              return (
                <tr key={p.id}>
                  <td>{p.name}</td>
                  <td className="mono">{p.path}</td>
                  <td>{p.defaultModel ?? "—"}</td>
                  <td>{active}</td>
                  <td>
                    <button className="btn small" onClick={() => remove(p.id)}>
                      Remove
                    </button>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
        <p className="muted small">
          Repository, branch, worktree and dirty-file details arrive with the Git integration (Phase 8).
        </p>
      </section>
    </div>
  );
}
