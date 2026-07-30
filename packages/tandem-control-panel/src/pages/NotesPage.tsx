import { useEffect, useState } from "react";
import { AnimatedPage, SplitView, EmptyState } from "../ui/index.tsx";
import {
  NotesList,
  loadNotes,
  notesStorageKey,
  saveNotes,
  type Note,
} from "../features/notes/NotesList";
import { NoteEditor, type NoteUpdate } from "../features/notes/NoteEditor";
import type { AppPageProps } from "./pageTypes";

const NOTES_LOAD_ERROR = "Notes could not be loaded for this account.";
const NOTES_SAVE_ERROR =
  "Notes could not be saved in this browser. Check storage permissions or available space.";

function createNoteId(): string {
  const browserCrypto = globalThis.crypto as Crypto | undefined;
  const uuid = browserCrypto?.randomUUID?.();
  if (uuid) return uuid;
  if (browserCrypto?.getRandomValues) {
    const bytes = new Uint32Array(4);
    browserCrypto.getRandomValues(bytes);
    return `note-${Array.from(bytes, (value) => value.toString(16).padStart(8, "0")).join("")}`;
  }
  return `note-${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
}

export function NotesPage({ principalId, toast }: AppPageProps) {
  const [notes, setNotes] = useState<Note[]>([]);
  const [selectedNoteId, setSelectedNoteId] = useState<string | null>(null);
  const [storageError, setStorageError] = useState<string | null>(null);
  const selectedNote = notes.find((note) => note.id === selectedNoteId) || null;

  useEffect(() => {
    setSelectedNoteId(null);
    let storageKey: string;
    try {
      storageKey = notesStorageKey(principalId);
      const storedNotes = loadNotes(principalId);
      setNotes(storedNotes);
      setStorageError(null);
    } catch {
      setNotes([]);
      setStorageError(NOTES_LOAD_ERROR);
      return;
    }

    const syncFromStorage = (event: StorageEvent) => {
      if (event.storageArea !== localStorage || event.key !== storageKey) return;
      try {
        const storedNotes = loadNotes(principalId);
        setNotes(storedNotes);
        setSelectedNoteId((currentId) =>
          currentId && storedNotes.some((note) => note.id === currentId) ? currentId : null
        );
        setStorageError(null);
      } catch {
        setStorageError(NOTES_LOAD_ERROR);
      }
    };
    window.addEventListener("storage", syncFromStorage);
    return () => window.removeEventListener("storage", syncFromStorage);
  }, [principalId]);

  const persistNotes = (mutate: (storedNotes: Note[]) => Note[]): boolean => {
    try {
      const nextNotes = mutate(loadNotes(principalId));
      saveNotes(principalId, nextNotes);
      setNotes(nextNotes);
      setSelectedNoteId((currentId) =>
        currentId && nextNotes.some((note) => note.id === currentId) ? currentId : null
      );
      setStorageError(null);
      return true;
    } catch {
      setStorageError(NOTES_SAVE_ERROR);
      toast("err", NOTES_SAVE_ERROR);
      return false;
    }
  };

  const createNote = () => {
    const now = Date.now();
    const note: Note = {
      id: createNoteId(),
      title: "New Note",
      content: "",
      createdAt: now,
      updatedAt: now,
    };
    if (
      persistNotes((storedNotes) => [
        note,
        ...storedNotes.filter((storedNote) => storedNote.id !== note.id),
      ])
    ) {
      setSelectedNoteId(note.id);
    }
  };

  const updateNote = (noteId: string, update: NoteUpdate) => {
    persistNotes((storedNotes) =>
      storedNotes.map((note) => (note.id === noteId ? { ...note, ...update } : note))
    );
  };

  const deleteNote = (noteId: string) => {
    if (
      persistNotes((storedNotes) => storedNotes.filter((note) => note.id !== noteId)) &&
      selectedNoteId === noteId
    ) {
      setSelectedNoteId(null);
    }
  };

  return (
    <AnimatedPage className="h-full min-h-0 flex flex-col gap-3">
      {storageError ? (
        <div
          role="alert"
          className="rounded-lg border border-red-400/30 bg-red-950/30 px-3 py-2 text-sm text-red-100"
        >
          {storageError}
        </div>
      ) : null}
      <SplitView
        className="flex-1 min-h-0"
        mainClassName="h-full min-h-0 flex"
        asideClassName="h-full min-h-0 flex"
        main={
          selectedNote ? (
            <NoteEditor note={selectedNote} onUpdate={updateNote} />
          ) : (
            <div className="flex-1">
              <EmptyState
                title="Select a note"
                text="Select a note from the sidebar or create a new one to get started"
              />
            </div>
          )
        }
        aside={
          <NotesList
            notes={notes}
            selectedNoteId={selectedNoteId}
            onCreateNote={createNote}
            onSelectNote={setSelectedNoteId}
            onDeleteNote={deleteNote}
          />
        }
      />
    </AnimatedPage>
  );
}
