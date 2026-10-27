// A list row (19, component list): the keys reach it through `data-row`, a click opens it, `c` copies its command.
import type { MouseEvent, ReactNode } from "react";
import { navigate } from "./router";

export function Row({
  href,
  command,
  className,
  children,
}: {
  href: string;
  command?: string;
  className?: string;
  children: ReactNode;
}) {
  const click = (e: MouseEvent<HTMLTableRowElement>) => {
    // A link or a chip inside the row acts for itself.
    if ((e.target as HTMLElement).closest("a, button")) return;
    navigate(href);
  };
  return (
    <tr data-row="" data-href={href} data-command={command} className={className} tabIndex={-1} onClick={click}>
      {children}
    </tr>
  );
}
