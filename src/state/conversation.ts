import { create } from "zustand";

import { conversationSave } from "@/lib/commands";
import type { Conversation } from "@/lib/ipc";
import { useTranscriptStore, withLiveArchived } from "@/state/transcript";

/**
 * The open (named, saved) conversation. While one is open, new listening
 * runs append to it on screen and re-saving replaces the stored record with
 * the fuller transcript — that's the append behavior. Library documents can
 * be linked to it; links persist with the record.
 */
interface ConversationState {
  /** Saved record id, once the conversation has been saved at least once. */
  openId: string | null;
  title: string | null;
  linkedDocs: string[];
  /** Stop offers to save; this drives the modal. */
  savePromptOpen: boolean;
  notice: string | null;

  setSavePromptOpen: (open: boolean) => void;
  setNotice: (notice: string | null) => void;
  /** Show a loaded conversation and make it the open one. */
  openConversation: (conversation: Conversation) => void;
  /** Close the current conversation and clear the screen. */
  newConversation: () => void;
  toggleLinkedDoc: (docId: string) => Promise<void>;
  /**
   * Persist the full on-screen transcript (archived runs + the live run's
   * finals) under the open conversation id, or create one.
   */
  save: (title?: string) => Promise<void>;
}

export const useConversationStore = create<ConversationState>((set, get) => ({
  openId: null,
  title: null,
  linkedDocs: [],
  savePromptOpen: false,
  notice: null,

  setSavePromptOpen: (open) => set({ savePromptOpen: open }),
  setNotice: (notice) => set({ notice }),

  openConversation: (conversation) => {
    const transcript = useTranscriptStore.getState();
    transcript.loadConversation(conversation.segments);
    transcript.setRetainHistory(true);
    set({
      openId: conversation.id,
      title: conversation.title,
      linkedDocs: conversation.linked_docs,
      notice: null,
    });
  },

  newConversation: () => {
    const transcript = useTranscriptStore.getState();
    transcript.clear();
    transcript.setRetainHistory(false);
    set({ openId: null, title: null, linkedDocs: [], notice: null });
  },

  toggleLinkedDoc: async (docId) => {
    const linked = get().linkedDocs.includes(docId)
      ? get().linkedDocs.filter((id) => id !== docId)
      : [...get().linkedDocs, docId];
    set({ linkedDocs: linked });
    // An already-saved conversation persists the link change immediately;
    // an unsaved one carries it into the first save.
    if (get().openId) {
      try {
        await get().save();
      } catch (e) {
        set({ notice: String(e) });
      }
    }
  },

  save: async (title) => {
    const transcript = useTranscriptStore.getState();
    const segments = withLiveArchived(transcript.archived, transcript.segments);
    const saved = await conversationSave(
      get().openId,
      title?.trim() || get().title,
      segments,
      get().linkedDocs,
    );
    transcript.setRetainHistory(true);
    set({
      openId: saved.id,
      title: saved.title,
      notice: `Saved "${saved.title}".`,
    });
  },
}));
