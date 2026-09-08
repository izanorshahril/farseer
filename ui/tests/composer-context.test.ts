import { describe, expect, test } from "bun:test";
import { selectedSubject, snapshotComposerContext, type ComposerAnchor, type SubjectSelection } from "../src/selection";

const subject: SubjectSelection = {
  project: "D:\\Dev\\project",
  conversation: "conversation-1",
  task: "task-1",
  run: "run-1",
  managerRunner: "claude-code",
};

describe("explicit composer context", () => {
  test("captures the displayed selection as an immutable value", () => {
    const anchor: ComposerAnchor = { widget: "Work", subject: "task-1" };
    const context = snapshotComposerContext(anchor, subject);

    expect(context).toEqual({
      widget: "Work",
      subject: "task-1",
      project: "D:\\Dev\\project",
      conversation: "conversation-1",
      task: "task-1",
      managerRunner: "claude-code",
    });
    expect(Object.isFrozen(context)).toBe(true);
  });

  test("ignores stale optional ids left on a widget anchor", () => {
    const anchor: ComposerAnchor = {
      widget: "Conversation",
      project: "old-project",
      conversation: "old-conversation",
      task: "old-task",
      managerRunner: "old-runner",
    };
    const context = snapshotComposerContext(anchor, {
      ...selectedSubject(),
      project: "new-project",
      conversation: "new-conversation",
      task: null,
      managerRunner: null,
    });

    expect(context.project).toBe("new-project");
    expect(context.conversation).toBe("new-conversation");
    expect(context.task).toBeNull();
    expect(context.managerRunner).toBeNull();
  });
});
