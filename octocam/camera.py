from __future__ import annotations

import io
import time
from typing import Any


def capture_jpeg(settings: dict[str, Any]) -> tuple[bytes | None, str | None]:
    try:
        from picamera2 import Picamera2
    except ImportError:
        return None, "Picamera2 is not installed on this system."

    try:
        from libcamera import Transform
    except ImportError:
        Transform = None

    width = int(settings["resolution_width"])
    height = int(settings["resolution_height"])
    hflip = bool(settings["hflip"]) or int(settings["rotation"]) == 180
    vflip = bool(settings["vflip"]) or int(settings["rotation"]) == 180

    camera = Picamera2()
    try:
        options: dict[str, Any] = {"main": {"size": (width, height)}}
        if Transform is not None:
            options["transform"] = Transform(hflip=hflip, vflip=vflip)

        config = camera.create_still_configuration(**options)
        camera.configure(config)
        camera.start()
        camera.set_controls(
            {
                "Brightness": int(settings["brightness"]) / 100,
                "Contrast": float(settings["contrast"]),
            }
        )
        time.sleep(0.35)

        stream = io.BytesIO()
        camera.capture_file(stream, format="jpeg")
        return stream.getvalue(), None
    except Exception as error:  # Picamera2 raises hardware/backend-specific errors.
        return None, str(error)
    finally:
        try:
            camera.close()
        except Exception:
            pass
