import { convertFileSrc } from "@tauri-apps/api/core";
import { useState } from "react";
import { ICON_BOX, type IconSize, Monogram } from "./Monogram";

/** The extracted 64px icon (24px by default), or a monogram when there is none or it fails to load. */
export function AppIcon({
  iconPath,
  name,
  size = 24,
}: {
  iconPath: string | null;
  name: string;
  size?: IconSize;
}) {
  const [broken, setBroken] = useState<string | null>(null);
  if (!iconPath || broken === iconPath) return <Monogram name={name} size={size} />;
  return (
    <img
      src={convertFileSrc(iconPath)}
      alt=""
      draggable={false}
      className={`shrink-0 object-contain ${ICON_BOX[size]}`}
      onError={() => setBroken(iconPath)}
    />
  );
}
