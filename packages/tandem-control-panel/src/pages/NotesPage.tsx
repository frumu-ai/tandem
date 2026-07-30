import { useLayoutEffect, useRef, useState } from "react";
import { AnimatedPage, SplitView, EmptyState } from "../ui/index.tsx";
import {
  InvalidStoredNotesError,
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
const NOTES_SAVE_DEBOUNCE_MS = 400;

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

function applyPendingNoteUpdates(
  notes: Note[],
  pendingUpdates: ReadonlyMap<string, NoteUpdate>
): Note[] {
  if (pendingUpdates.size === 0) return notes;
  return notes.map((note) => {
    const update = pendingUpdates.get(note.id);
    return update
      ? { ...note, ...update, updatedAt: Math.max(note.updatedAt, update.updatedAt) }
      : note;
  });
}

export function NotesPage({ principalId, toast }: AppPageProps) {
  const [notes, setNotes] = useState<Note[]>([]);
  const [selectedNoteId, setSelectedNoteId] = useState<string | null>(null);
  const [storageError, setStorageError] = useState<string | null>(null);
  const [hasInvalidStoredNotes, setHasInvalidStoredNotes] = useState(false);
  const persistedNotesRef = useRef<Note[]>([]);
  const pendingNoteUpdatesRef = useRef<Map<string, NoteUpdate>>(new Map());
  const pendingSaveTimerRef = useRef<number | null>(null);
  const toastRef = useRef(toast);
  toastRef.current = toast;
  const selectedNote = notes.find((note) => note.id === selectedNoteId) || null;

  useLayoutEffect(() => {
    setSelectedNoteId(null);
    let storageKey: string;
    try {
      storageKey = notesStorageKey(principalId);
    } catch {
      persistedNotesRef.current = [];
      setNotes([]);
      setStorageError(NOTES_LOAD_ERROR);
      setHasInvalidStoredNotes(false);
      return;
    }

    try {
      const storedNotes = loadNotes(principalId);
      persistedNotesRef.current = storedNotes;
      setNotes(storedNotes);
      setStorageError(null);
      setHasInvalidStoredNotes(false);
    } catch (error) {
      persistedNotesRef.current = [];
      setNotes([]);
      setStorageError(NOTES_LOAD_ERROR);
      setHasInvalidStoredNotes(error instanceof InvalidStoredNotesError);
    }

    const syncFromStorage = (event: StorageEvent) => {
      if (
        event.storageArea !== localStorage ||
        (event.key !== storageKey && event.key !== null)
      ) {
        return;
      }
      try {
        const storedNotes = loadNotes(principalId);
        persistedNotesRef.current = storedNotes;
        const storedNoteIds = new Set(storedNotes.map((note) => note.id));
        for (const noteId of pendingNoteUpdatesRef.current.keys()) {
          if (!storedNoteIds.has(noteId)) pendingNoteUpdatesRef.current.delete(noteId);
        }
        if (
          pendingNoteUpdatesRef.current.size === 0 &&
          pendingSaveTimerRef.current !== null
        ) {
          window.clearTimeout(pendingSaveTimerRef.current);
          pendingSaveTimerRef.current = null;
        }
        const renderedNotes = applyPendingNoteUpdates(
          storedNotes,
          pendingNoteUpdatesRef.current
        );
        setNotes(renderedNotes);
        setSelectedNoteId((currentId) =>
          currentId && renderedNotes.some((note) => note.id === currentId) ? currentId : null
        );
        setStorageError(null);
        setHasInvalidStoredNotes(false);
      } catch (error) {
        if (pendingSaveTimerRef.current !== null) {
          window.clearTimeout(pendingSaveTimerRef.current);
          pendingSaveTimerRef.current = null;
        }
        pendingNoteUpdatesRef.current.clear();
        const retainedNotes = persistedNotesRef.current;
        setNotes(retainedNotes);
        setSelectedNoteId((currentId) =>
          currentId && retainedNotes.some((note) => note.id === currentId) ? currentId : null
        );
        setStorageError(NOTES_LOAD_ERROR);
        setHasInvalidStoredNotes(error instanceof InvalidStoredNotesError);
      }
    };
    const flushPendingUpdatesBeforeTeardown = (retainOnFailure: boolean) => {
      if (pendingSaveTimerRef.current !== null) {
        window.clearTimeout(pendingSaveTimerRef.current);
        pendingSaveTimerRef.current = null;
      }
      const pendingUpdates = new Map(pendingNoteUpdatesRef.current);
      if (pendingUpdates.size === 0) return;
      try {
        const storedNotes = loadNotes(principalId);
        const nextNotes = applyPendingNoteUpdates(storedNotes, pendingUpdates);
        saveNotes(principalId, nextNotes);
        persistedNotesRef.current = nextNotes;
        pendingNoteUpdatesRef.current.clear();
      } catch (error) {
        if (!retainOnFailure) pendingNoteUpdatesRef.current.clear();
        const message =
          error instanceof InvalidStoredNotesError ? NOTES_LOAD_ERROR : NOTES_SAVE_ERROR;
        toastRef.current("err", message);
      }
    };
    const handlePageHide = () => flushPendingUpdatesBeforeTeardown(true);
    window.addEventListener("storage", syncFromStorage);
    window.addEventListener("pagehide", handlePageHide);
    return () => {
      window.removeEventListener("storage", syncFromStorage);
      window.removeEventListener("pagehide", handlePageHide);
      flushPendingUpdatesBeforeTeardown(false);
    };
  }, [principalId]);

  const commitNotes = (
    mutate?: (storedNotes: Note[]) => Note[],
    discardPendingNoteId?: string
  ): boolean => {
    if (pendingSaveTimerRef.current !== null) {
      window.clearTimeout(pendingSaveTimerRef.current);
      pendingSaveTimerRef.current = null;
    }
    if (discardPendingNoteId) pendingNoteUpdatesRef.current.delete(discardPendingNoteId);
    const pendingUpdates = new Map(pendingNoteUpdatesRef.current);
    pendingNoteUpdatesRef.current.clear();
    if (!mutate && pendingUpdates.size === 0) return true;

    let storedNotes: Note[];
    try {
      storedNotes = loadNotes(principalId);
    } catch (error) {
      const invalidStoredNotes = error instanceof InvalidStoredNotesError;
      const rollbackNotes = persistedNotesRef.current;
      setNotes(rollbackNotes);
      setSelectedNoteId((currentId) =>
        currentId && rollbackNotes.some((note) => note.id === currentId) ? currentId : null
      );
      const message = invalidStoredNotes ? NOTES_LOAD_ERROR : NOTES_SAVE_ERROR;
      setStorageError(message);
      setHasInvalidStoredNotes(invalidStoredNotes);
      toast("err", message);
      return false;
    }

    const reconciledNotes = applyPendingNoteUpdates(storedNotes, pendingUpdates);
    const nextNotes = mutate ? mutate(reconciledNotes) : reconciledNotes;
    try {
      saveNotes(principalId, nextNotes);
    } catch {
      persistedNotesRef.current = storedNotes;
      setNotes(storedNotes);
      setSelectedNoteId((currentId) =>
        currentId && storedNotes.some((note) => note.id === currentId) ? currentId : null
      );
      setStorageError(NOTES_SAVE_ERROR);
      setHasInvalidStoredNotes(false);
      toast("err", NOTES_SAVE_ERROR);
      return false;
    }

    persistedNotesRef.current = nextNotes;
    setNotes(nextNotes);
    setSelectedNoteId((currentId) =>
      currentId && nextNotes.some((note) => note.id === currentId) ? currentId : null
    );
    setStorageError(null);
    setHasInvalidStoredNotes(false);
    return true;
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
      commitNotes((storedNotes) => [
        note,
        ...storedNotes.filter((storedNote) => storedNote.id !== note.id),
      ])
    ) {
      setSelectedNoteId(note.id);
    }
  };

  const updateNote = (noteId: string, update: NoteUpdate) => {
    const currentUpdate = pendingNoteUpdatesRef.current.get(noteId);
    pendingNoteUpdatesRef.current.set(noteId, {
      ...(currentUpdate || {}),
      ...update,
      updatedAt: Math.max(currentUpdate?.updatedAt || 0, update.updatedAt),
    });
    setNotes((currentNotes) =>
      currentNotes.map((note) => (note.id === noteId ? { ...note, ...update } : note))
    );
    if (pendingSaveTimerRef.current !== null) {
      window.clearTimeout(pendingSaveTimerRef.current);
    }
    pendingSaveTimerRef.current = window.setTimeout(() => {
      pendingSaveTimerRef.current = null;
      commitNotes();
    }, NOTES_SAVE_DEBOUNCE_MS);
  };

  const flushPendingUpdates = () => {
    commitNotes();
  };

  const selectNote = (noteId: string) => {
    if (commitNotes()) setSelectedNoteId(noteId);
  };

  const deleteNote = (noteId: string) => {
    const note = notes.find((candidate) => candidate.id === noteId);
    const title = note?.title || "Untitled Note";
    if (!window.confirm(`Delete "${title}"? This cannot be undone.`)) return;
    commitNotes(
      (storedNotes) => storedNotes.filter((storedNote) => storedNote.id !== noteId),
      noteId
    );
  };

  const resetLocalNotes = () => {
    if (
      !window.confirm(
        "Reset all local notes for this account? This permanently removes the corrupted data."
      )
    ) {
      return;
    }
    if (pendingSaveTimerRef.current !== null) {
      window.clearTimeout(pendingSaveTimerRef.current);
      pendingSaveTimerRef.current = null;
    }
    pendingNoteUpdatesRef.current.clear();
    try {
      localStorage.removeItem(notesStorageKey(principalId));
      persistedNotesRef.current = [];
      setNotes([]);
      setSelectedNoteId(null);
      setStorageError(null);
      setHasInvalidStoredNotes(false);
    } catch {
      setStorageError(NOTES_SAVE_ERROR);
      setHasInvalidStoredNotes(true);
      toast("err", NOTES_SAVE_ERROR);
    }
  };

  return (
    <AnimatedPage className="h-full min-h-0 flex flex-col gap-3">
      {storageError ? (
        <div
          role="alert"
          className="rounded-lg border border-red-400/30 bg-red-950/30 px-3 py-2 text-sm text-red-100"
        >
          <div>{storageError}</div>
          {hasInvalidStoredNotes ? (
            <button
              type="button"
              className="tcp-btn mt-2"
              onClick={resetLocalNotes}
            >
              Reset local notes
            </button>
          ) : null}
        </div>
      ) : null}
      <SplitView
        className="flex-1 min-h-0"
        mainClassName="h-full min-h-0 flex"
        asideClassName="h-full min-h-0 flex"
        main={
          selectedNote ? (
            <NoteEditor note={selectedNote} onUpdate={updateNote} onFlush={flushPendingUpdates} />
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
            onSelectNote={selectNote}
            onDeleteNote={deleteNote}
          />
        }
      />
    </AnimatedPage>
  );
}
