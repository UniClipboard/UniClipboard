"use client";
import * as React from "react";
import { ArrowDownToLine, ArrowUpToLine, ChevronDown, ChevronUp, RotateCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import { LAYOUT_HINT, LAYOUT_LABEL } from "@/lib/constants";
import { pickText, writeLocalized } from "@/lib/locale";
import type { ElementId, ElementTransform, Slide, SlideLayout } from "@/lib/types";
import { ScreenshotPicker } from "./screenshot-picker";

type Props = {
  slide: Slide;
  locale: string;
  selectedElementId: ElementId | null;
  onChange: (patch: Partial<Slide>) => void;
};

const ELEMENT_LABEL: Record<ElementId, string> = {
  caption: "Headline",
  device: "Device",
  deviceSecondary: "Back device",
};

export function Inspector({ slide, locale, selectedElementId, onChange }: Props) {
  const isFeatureGraphic = slide.layout === "feature-graphic";
  const isNoDevice = slide.layout === "no-device";
  const localeLabel = slide.label?.[locale] ?? "";
  const localeHeadline = slide.headline?.[locale] ?? "";
  // When the active locale is empty, surface the fallback (typically en) as
  // the placeholder so the user sees what they're translating from.
  const headlineDefault = isFeatureGraphic ? "Your tagline." : "One idea\nper slide.";
  const labelPlaceholder = localeLabel ? "FEATURE 01" : pickText(slide.label, locale) || "FEATURE 01";
  const headlinePlaceholder = localeHeadline
    ? headlineDefault
    : pickText(slide.headline, locale) || headlineDefault;

  function setLocaleField(key: "label" | "headline", value: string) {
    onChange({ [key]: writeLocalized(slide[key], locale, value) } as Partial<Slide>);
  }

  return (
    <div className="flex h-full flex-col">
      <div className="border-b p-3">
        <div className="flex items-baseline justify-between gap-2">
          <h2 className="text-sm font-semibold">Slide settings</h2>
          <span className="text-[10px] uppercase tracking-wide text-muted-foreground">
            editing · {locale.toUpperCase()}
          </span>
        </div>
        <p className="text-xs text-muted-foreground">{LAYOUT_HINT[slide.layout]}</p>
      </div>

      <div className="flex-1 space-y-4 overflow-y-auto p-3">
        <div className="space-y-1.5">
          <Label className="text-xs">Layout</Label>
          <Select
            value={slide.layout}
            onValueChange={(layout) => {
              const next = layout as SlideLayout;
              onChange({
                layout: next,
                transforms: undefined,
                screenshotSecondary:
                  next === "two-devices" ? slide.screenshotSecondary || slide.screenshot : undefined,
              });
            }}
          >
            <SelectTrigger>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {Object.entries(LAYOUT_LABEL).map(([layout, label]) => (
                <SelectItem key={layout} value={layout}>
                  {label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>

        {!isFeatureGraphic && (
          <div className="space-y-1.5">
            <Label className="text-xs">Label</Label>
            <Input
              value={localeLabel}
              onChange={(e) => setLocaleField("label", e.target.value)}
              placeholder={labelPlaceholder}
            />
          </div>
        )}

        <div className="space-y-1.5">
          <div className="flex items-baseline justify-between">
            <Label className="text-xs">{isFeatureGraphic ? "Tagline" : "Headline"}</Label>
            <span className="text-[10px] text-muted-foreground">newline = break</span>
          </div>
          <Textarea
            value={localeHeadline}
            onChange={(e) => setLocaleField("headline", e.target.value)}
            rows={3}
            placeholder={headlinePlaceholder}
          />
        </div>

        {!isFeatureGraphic && !isNoDevice && (
          <div className="space-y-1.5">
            <Label className="text-xs">
              {slide.layout === "two-devices" ? "Front device screenshot" : "Screenshot"}
            </Label>
            <ScreenshotPicker
              label="Primary"
              value={slide.screenshot}
              onChange={(v) => onChange({ screenshot: v })}
            />
          </div>
        )}

        {slide.layout === "two-devices" && (
          <div className="space-y-1.5">
            <Label className="text-xs">Back device screenshot</Label>
            <ScreenshotPicker
              label="Secondary (back layer)"
              value={slide.screenshotSecondary || ""}
              onChange={(v) => onChange({ screenshotSecondary: v })}
            />
          </div>
        )}

        {!isFeatureGraphic && (
          <ElementTransformControls
            slide={slide}
            selectedElementId={selectedElementId}
            onChange={onChange}
          />
        )}

        {isFeatureGraphic && (
          <p className="rounded-md border bg-muted/40 p-3 text-[11px] leading-relaxed text-muted-foreground">
            Shows app icon + name + tagline. Drop an icon at <span className="rounded bg-background px-1 py-0.5 font-mono text-[10px] text-foreground">/public/app-icon.png</span> (or leave blank — the app initial will be used). Name is set in the toolbar.
          </p>
        )}
      </div>
    </div>
  );
}

function ElementTransformControls({
  slide,
  selectedElementId,
  onChange,
}: {
  slide: Slide;
  selectedElementId: ElementId | null;
  onChange: (patch: Partial<Slide>) => void;
}) {
  const present: ElementId[] = ["caption"];
  if (slide.layout !== "no-device") present.push("device");
  if (slide.layout === "two-devices") present.push("deviceSecondary");

  const transforms = slide.transforms || {};
  const activeId =
    selectedElementId && present.includes(selectedElementId) ? selectedElementId : null;

  function patchElement(id: ElementId, patch: Partial<ElementTransform>) {
    const cur = transforms[id];
    if (!cur) return; // can only adjust after user moves/resizes (default rect lives in slide-canvas)
    onChange({
      transforms: { ...transforms, [id]: { ...cur, ...patch } },
    });
  }

  // Z-order: re-rank zIndex among present elements so they remain contiguous.
  function reorder(id: ElementId, dir: "front" | "back" | "up" | "down") {
    const ranked = [...present].sort((a, b) => {
      const za = transforms[a]?.zIndex ?? defaultZ(a);
      const zb = transforms[b]?.zIndex ?? defaultZ(b);
      return za - zb;
    });
    const idx = ranked.indexOf(id);
    if (idx === -1) return;
    let target = idx;
    if (dir === "front") target = ranked.length - 1;
    else if (dir === "back") target = 0;
    else if (dir === "up") target = Math.min(ranked.length - 1, idx + 1);
    else if (dir === "down") target = Math.max(0, idx - 1);
    if (target === idx) return;
    ranked.splice(idx, 1);
    ranked.splice(target, 0, id);
    const next = { ...transforms };
    ranked.forEach((eid, i) => {
      const cur = next[eid];
      if (!cur) return; // skip if user hasn't engaged this element yet
      next[eid] = { ...cur, zIndex: i + 1 };
    });
    onChange({ transforms: next });
  }

  return (
    <div className="space-y-3 rounded-md border bg-muted/30 p-3">
      <div>
        <Label className="text-xs font-semibold">Elements</Label>
        <p className="text-[11px] text-muted-foreground">
          {activeId
            ? "Fine-tune the selected element's rotation and stacking."
            : "Click an element on the canvas to fine-tune its rotation and stacking."}
        </p>
      </div>

      {activeId ? (
        <ActiveElementPanel
          activeId={activeId}
          transform={transforms[activeId]}
          onRotate={(rotation) => patchElement(activeId, { rotation })}
          onReorder={(dir) => reorder(activeId, dir)}
        />
      ) : (
        <div className="rounded border border-dashed bg-background/40 p-4 text-center text-[11px] text-muted-foreground">
          No element selected
        </div>
      )}
    </div>
  );
}

function ActiveElementPanel({
  activeId,
  transform,
  onRotate,
  onReorder,
}: {
  activeId: ElementId;
  transform: ElementTransform | undefined;
  onRotate: (rotation: number) => void;
  onReorder: (dir: "front" | "back" | "up" | "down") => void;
}) {
  const engaged = !!transform;
  const rotation = transform?.rotation ?? 0;
  const label = ELEMENT_LABEL[activeId];
  return (
    <div className="space-y-2 rounded border bg-background/60 p-2.5">
      <div className="flex items-center justify-between">
        <span className="text-xs font-medium">{label}</span>
        {!engaged && (
          <span className="text-[10px] text-muted-foreground">drag to enable</span>
        )}
      </div>

      <div className="space-y-1">
        <div className="flex items-center justify-between">
          <Label className="flex items-center gap-1 text-[11px] text-muted-foreground">
            <RotateCw className="h-3 w-3" /> Rotation
          </Label>
          <span className="text-[11px] tabular-nums text-muted-foreground">
            {rotation}°
          </span>
        </div>
        <input
          type="range"
          min={-180}
          max={180}
          step={1}
          value={rotation}
          disabled={!engaged}
          onChange={(e) => onRotate(Number(e.target.value))}
          className="w-full disabled:opacity-50"
          aria-label={`${label} rotation`}
        />
      </div>

      <div className="space-y-1">
        <Label className="text-[11px] text-muted-foreground">Layer</Label>
        <div className="grid grid-cols-4 gap-1">
          <LayerButton disabled={!engaged} onClick={() => onReorder("back")} label="Send to back">
            <ArrowDownToLine className="h-3.5 w-3.5" />
          </LayerButton>
          <LayerButton disabled={!engaged} onClick={() => onReorder("down")} label="Send backward">
            <ChevronDown className="h-3.5 w-3.5" />
          </LayerButton>
          <LayerButton disabled={!engaged} onClick={() => onReorder("up")} label="Bring forward">
            <ChevronUp className="h-3.5 w-3.5" />
          </LayerButton>
          <LayerButton disabled={!engaged} onClick={() => onReorder("front")} label="Bring to front">
            <ArrowUpToLine className="h-3.5 w-3.5" />
          </LayerButton>
        </div>
      </div>
    </div>
  );
}

function LayerButton({
  disabled,
  onClick,
  label,
  children,
}: {
  disabled: boolean;
  onClick: () => void;
  label: string;
  children: React.ReactNode;
}) {
  return (
    <Button
      type="button"
      variant="outline"
      size="sm"
      className="h-7 px-0"
      disabled={disabled}
      onClick={onClick}
      title={label}
      aria-label={label}
    >
      {children}
    </Button>
  );
}

function defaultZ(id: ElementId): number {
  if (id === "deviceSecondary") return 2;
  if (id === "device") return 3;
  return 4; // caption on top
}
