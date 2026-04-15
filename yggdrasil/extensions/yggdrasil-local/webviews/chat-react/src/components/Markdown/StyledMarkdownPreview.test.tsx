import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import { StyledMarkdownPreview } from "./StyledMarkdownPreview";

// vscode module stub — the CodeBlock uses `post` from ../../vscode.
vi.mock("../../vscode", () => ({
  post: vi.fn(),
}));

describe("StyledMarkdownPreview", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders inline code as plain <code>, not a CodeBlock", () => {
    render(<StyledMarkdownPreview markdown="Hello `inline` world" />);
    const code = screen.getByText("inline");
    expect(code.tagName).toBe("CODE");
    // Inline <code> has no Copy/Apply toolbar.
    expect(screen.queryByLabelText("Copy code")).toBeNull();
  });

  it("renders fenced code with a Copy button", () => {
    const md = "```rust\nfn main() {}\n```";
    render(<StyledMarkdownPreview markdown={md} />);
    expect(screen.getByLabelText("Copy code")).toBeDefined();
    expect(screen.queryByText(/Apply edit/)).toBeNull();
  });

  it("treats `yggdrasil-edit:<path>` fences as editable and shows Apply button", () => {
    const md = "```yggdrasil-edit:/tmp/foo.rs\nfn main() {}\n```";
    render(<StyledMarkdownPreview markdown={md} />);
    expect(screen.getByLabelText(/Apply edit to \/tmp\/foo\.rs/)).toBeDefined();
    expect(screen.getByLabelText("Copy code")).toBeDefined();
  });

  it("renders tables from GFM", () => {
    const md = "| a | b |\n|---|---|\n| 1 | 2 |";
    render(<StyledMarkdownPreview markdown={md} />);
    expect(screen.getByText("a")).toBeDefined();
    expect(screen.getByText("2")).toBeDefined();
  });
});
