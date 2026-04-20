import { useState } from "react";

type BannerProps = {
  title: string;
  emphasis?: "low" | "high";
};

export function Banner({ title, emphasis = "low" }: BannerProps) {
  const [open, setOpen] = useState(true);

  return (
    <section data-emphasis={emphasis}>
      <button onClick={() => setOpen((value) => !value)}>{title}</button>
      {open ? <span>visible</span> : null}
    </section>
  );
}
