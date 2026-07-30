import { PanelCard, EmptyState, IconButton, Icon } from "../../ui/index.tsx";

export type Note = {
  id: string;
  title: string;
  content: string;
  createdAt: number;
  updatedAt: number;
};

const STORAGE_KEY_PREFIX = "tandem-control-panel-notes";

export class InvalidStoredNotesError extends Error {
  constructor() {
    super("Stored notes are invalid.");
    this.name = "InvalidStoredNotesError";
  }
}

export function notesStorageKey(principalId: string): string {
  const normalizedPrincipalId = principalId.trim();
  if (!normalizedPrincipalId) {
    throw new Error("A principal ID is required to access notes.");
  }
  return `${STORAGE_KEY_PREFIX}:${encodeURIComponent(normalizedPrincipalId)}`;
}

function isNote(value: unknown): value is Note {
  if (!value || typeof value !== "object") return false;
  const note = value as Partial<Note>;
  return (
    typeof note.id === "string" &&
    note.id.trim().length > 0 &&
    typeof note.title === "string" &&
    typeof note.content === "string" &&
    typeof note.createdAt === "number" &&
    Number.isFinite(note.createdAt) &&
    typeof note.updatedAt === "number" &&
    Number.isFinite(note.updatedAt)
  );
}

export function loadNotes(principalId: string): Note[] {
  const stored = localStorage.getItem(notesStorageKey(principalId));
  if (stored === null) return [];
  let parsed: unknown;
  try {
    parsed = JSON.parse(stored);
  } catch {
    throw new InvalidStoredNotesError();
  }
  if (!Array.isArray(parsed) || !parsed.every(isNote)) {
    throw new InvalidStoredNotesError();
  }
  if (new Set(parsed.map((note) => note.id)).size !== parsed.length) {
    throw new InvalidStoredNotesError();
  }
  return parsed;
}

export function saveNotes(principalId: string, notes: Note[]): void {
  localStorage.setItem(notesStorageKey(principalId), JSON.stringify(notes));
}

type NotesListProps = {
  notes: Note[];
  selectedNoteId: string | null;
  onCreateNote: () => void;
  onSelectNote: (id: string) => void;
  onDeleteNote: (id: string) => void;
};

export function NotesList({
  notes,
  selectedNoteId,
  onCreateNote,
  onSelectNote,
  onDeleteNote,
}: NotesListProps) {
  return (
    <PanelCard
      title="Notes"
      actions={
        <button type="button" className="tcp-btn-primary" onClick={onCreateNote}>
          <Icon name="plus" />
          New Note
        </button>
      }
      fullHeight
    >
      <div className="flex-1 overflow-auto min-h-0">
        {notes.length === 0 ? (
          <EmptyState
            text="Create your first note to get started"
            action={
              <button type="button" className="tcp-btn-primary" onClick={onCreateNote}>
                Create Note
              </button>
            }
          />
        ) : (
          <div className="grid gap-2">
            {notes.map((note) => (
              <div
                key={note.id}
                className="flex items-center justify-between gap-3 rounded-lg border border-white/6 bg-white/3 p-3 hover:bg-white/6"
              >
                <button
                  type="button"
                  className="min-w-0 flex-1 text-left"
                  aria-label={`Open note ${note.title || "Untitled Note"}`}
                  aria-pressed={selectedNoteId === note.id}
                  onClick={() => onSelectNote(note.id)}
                >
                  <div className="font-semibold truncate">
                    {note.title || "Untitled Note"}
                  </div>
                  <div className="text-xs tcp-subtle mt-0.5 truncate">
                    {note.content || "No content"}
                  </div>
                </button>
                <IconButton
                  aria-label={`Delete note ${note.title || "Untitled Note"}`}
                  onClick={() => onDeleteNote(note.id)}
                >
                  <Icon name="trash-2" />
                </IconButton>
              </div>
            ))}
          </div>
        )}
      </div>
    </PanelCard>
  );
}
