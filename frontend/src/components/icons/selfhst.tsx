import type { SVGProps } from "react"

// Brand glyphs vendored from the selfh.st icon set (https://selfh.st/icons/).
//
// We inline the icons instead of pulling in an icon-library dependency so the
// bundle stays lean for the Pi and nothing is fetched at runtime. For each logo
// we use the `-dark` variant, because that variant ships as a single path with
// NO baked-in fill — which lets us bind it to `currentColor` and have it inherit
// the surrounding text color (sidebar muted → foreground + active state, or the
// page-header `text-primary`) in both light and dark mode, exactly like the
// lucide icons it sits next to. The `-light`/base variants hardcode #fff / brand
// colors and wouldn't theme.
//
// To add another selfhst icon, grab its `-dark` SVG's path and add a component
// here following the same shape.

type IconProps = SVGProps<SVGSVGElement>

// selfhst:matter (dark variant geometry, bound to currentColor)
export function MatterIcon(props: IconProps) {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      width={24}
      height={24}
      viewBox="0 0 512 512"
      fill="currentColor"
      aria-hidden="true"
      {...props}
    >
      <path d="M152 134.4c21.5 17.5 47.1 29.2 74.4 34.2V22.9l29.7-17.1 29.6 17.1v145.6c27.3-4.9 52.9-16.7 74.5-34.2l53.8 31.1c-87.6 86.6-228.5 86.6-316.1 0zm65.5 371.8C248.7 387 178.1 264.9 59.3 232.5v62.3c25.9 9.9 48.9 26.2 66.8 47.4L0 414.9v34.2l29.7 17 126.1-72.8c9.4 26.1 12 54.2 7.6 81.5zm235.3-273.7C334 265 263.6 387.1 294.8 506.2l54-31.2c-4.4-27.4-1.7-55.4 7.6-81.5l126 72.7 29.6-17.1v-34.2l-126.1-72.8c17.9-21.2 40.9-37.5 66.8-47.4z" />
    </svg>
  )
}

// selfhst:apple-homekit (dark variant geometry, bound to currentColor)
export function AppleHomeKitIcon(props: IconProps) {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      width={24}
      height={24}
      viewBox="0 0 512 512"
      fill="currentColor"
      aria-hidden="true"
      {...props}
    >
      <path d="m506.8 213.8-47.6-37.6V90.8c0-5.8-2.3-7.4-6.4-7.4h-43.5c-4.7 0-7.6.9-7.6 7.4v39.9C338.8 81 270.6 27.2 268 25.2c-5.1-4.1-8.3-5.1-12-5.1-3.6 0-6.8 1.1-12 5.1S12 208.4 5.2 213.8c-8.4 6.6-6 16.2 3.3 16.2h44.3v240.3c0 15.5 6.2 21.8 21 21.8h364.5c14.8 0 21-6.2 21-21.8V229.8h44.3c9.3 0 11.6-9.4 3.2-16M422 436.4c0 10.7-5.5 18.2-16.8 18.2H106.7c-11.2 0-16.8-7.4-16.8-18.2V211.9c0-13 5.7-21.4 12.2-26.5L245.7 72.1c3.8-3 7-4.3 10.2-4.3s6.4 1.3 10.2 4.3l143.5 113.3c6.5 5.1 12.2 13.4 12.2 26.5zm-46.5-229.8c-3.9-3-108.3-85.6-111.1-87.7-2.8-2.2-5.7-3.3-8.4-3.3s-5.7 1.1-8.4 3.3c-2.8 2.2-107.2 84.7-111.1 87.7-6.9 5.5-9.2 11.1-9.2 20.7v175.4c0 10 5.7 14.7 14 14.7h229.6c8.3 0 14-4.7 14-14.7V227.3c-.2-9.6-2.5-15.2-9.4-20.7m-28 162.4c0 8-4.5 11.2-10.5 11.2H175c-6.1 0-10.5-3.1-10.5-11.2V242.9c0-5.6 0-10.1 6.1-15 4.1-3.2 76.5-60.4 78.7-62.1s4.3-2.6 6.7-2.6c2.4.1 4.8 1 6.7 2.6 2.2 1.7 74.6 58.9 78.7 62.1 6.1 4.9 6.1 9.4 6.1 15zM256 342.8h46.8c4.3 0 7.4-1.4 7.4-7.6v-76.9c0-4.3-2-8.5-5.3-11.2-1.9-1.6-42.6-33.4-44-34.5-2.8-2.4-7-2.4-9.9 0-1.4 1.1-42.1 33-44 34.5-3.4 2.8-5.3 6.9-5.3 11.2v76.9c0 6.2 3.1 7.6 7.4 7.6z" />
    </svg>
  )
}
