/** Iconos de línea monocromos (mismo estilo que RackMap): toman el color del texto. */
const PATHS = {
  plus: "M10 4v12M4 10h12",
  gear: "M10 7a3 3 0 1 0 0 6 3 3 0 0 0 0-6zM10 2.5v2M10 15.5v2M2.5 10h2M15.5 10h2M4.7 4.7l1.4 1.4M13.9 13.9l1.4 1.4M4.7 15.3l1.4-1.4M13.9 6.1l1.4-1.4",
  search: "M8.5 3.5a5 5 0 1 0 0 10 5 5 0 0 0 0-10zM12.2 12.2 16.5 16.5",
  trash: "M3.5 5.5h13M8 5.5V3.5h4v2M5.5 5.5l.8 11h7.4l.8-11M8.5 8.5v5M11.5 8.5v5",
  close: "M5.5 5.5l9 9M14.5 5.5l-9 9",
  chat: "M3.5 4.5h13v9h-7l-4 3v-3h-2z",
  mic: "M10 2.75a2.5 2.5 0 0 0-2.5 2.5v4.5a2.5 2.5 0 0 0 5 0v-4.5a2.5 2.5 0 0 0-2.5-2.5zM5 9.5a5 5 0 0 0 10 0M10 14.5v2.75",
  send: "M10 16V4M5 9l5-5 5 5",
  stop: "M6 6h8v8H6z",
  lock: "M5.5 9h9v7.5h-9zM7.5 9V6.5a2.5 2.5 0 0 1 5 0V9",
} as const;

export type IconName = keyof typeof PATHS;

export function Icon({ name, size = 18 }: { name: IconName; size?: number }) {
  return (
    <svg className="icon-line" width={size} height={size} viewBox="0 0 20 20" aria-hidden fill="none"
      stroke="currentColor" strokeWidth={1.4} strokeLinecap="round" strokeLinejoin="round">
      <path d={PATHS[name]} />
    </svg>
  );
}
