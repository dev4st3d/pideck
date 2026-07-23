import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";

export default function stockExtensionUiDemo(pi: ExtensionAPI) {
  pi.on("session_start", (_event, ctx) => {
    ctx.ui.setTitle("Pi stock extension UI demo");
    ctx.ui.setStatus("stock-demo", "Ready");
    ctx.ui.setWidget("stock-above", ["Above the composer"], {
      placement: "aboveEditor",
    });
    ctx.ui.setWidget("stock-below", ["Below the composer"], {
      placement: "belowEditor",
    });
    ctx.ui.notify("Stock extension UI demo loaded", "info");
  });

  pi.registerCommand("stock-ui-dialogs", {
    description: "Exercise every stock RPC dialog request",
    handler: async (_args, ctx) => {
      const selected = await ctx.ui.select(
        "Choose a demo option",
        ["Alpha", "Beta"],
        { timeout: 30_000 },
      );
      const confirmed = await ctx.ui.confirm(
        "Confirm the demo",
        "This is untrusted extension content.",
        { timeout: 30_000 },
      );
      const input = await ctx.ui.input(
        "Enter demo text",
        "Type a value",
        { timeout: 30_000 },
      );
      const edited = await ctx.ui.editor("Edit demo text", "Line 1\nLine 2");
      ctx.ui.notify(
        `select=${selected ?? "cancelled"} confirm=${confirmed} input=${input ?? "cancelled"} editor=${edited ?? "cancelled"}`,
        "info",
      );
    },
  });

  pi.registerCommand("stock-ui-replace-clear", {
    description: "Exercise keyed status and widget replacement and clearing",
    handler: async (_args, ctx) => {
      ctx.ui.setStatus("stock-demo", "Replacing");
      ctx.ui.setStatus("stock-demo", "Replaced");
      ctx.ui.setWidget("stock-above", ["Replacement line"]);
      ctx.ui.setWidget("stock-below", undefined);
      ctx.ui.setStatus("stock-demo", undefined);
      ctx.ui.setWidget("stock-above", undefined);
      ctx.ui.setTitle("Pi stock demo updated");
      ctx.ui.setEditorText("Draft supplied by the stock demo extension.");
      ctx.ui.notify("Extension UI state updated", "info");
    },
  });

  pi.registerCommand("stock-ui-unsupported", {
    description: "Demonstrate stock RPC's explicit TUI-only degradation",
    handler: async (_args, ctx) => {
      const customResult = await ctx.ui.custom(() => undefined as never);
      ctx.ui.setWidget("component-widget", () => undefined as never);
      ctx.ui.setHeader(() => undefined as never);
      ctx.ui.setFooter(() => undefined as never);
      ctx.ui.setEditorComponent(() => undefined as never);
      const themeResult = ctx.ui.setTheme("not-transported");
      ctx.ui.notify(
        `custom=${String(customResult)} theme=${themeResult.success ? "unexpected" : "unsupported"}`,
        "warning",
      );
    },
  });
}
