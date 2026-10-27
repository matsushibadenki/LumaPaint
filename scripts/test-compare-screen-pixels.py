import importlib.util
from pathlib import Path
import tempfile
import unittest

from PIL import Image, ImageCms


spec = importlib.util.spec_from_file_location("compare_screen", Path(__file__).with_name("compare-screen-pixels.py"))
compare_screen = importlib.util.module_from_spec(spec)
spec.loader.exec_module(compare_screen)


class CompareScreenPixelsTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.path = Path(self.temp.name)
        self.profile = ImageCms.ImageCmsProfile(ImageCms.createProfile("sRGB")).tobytes()

    def save(self, name, pixels, *, profile=None, mode="RGB"):
        image = Image.new(mode, (2, 1))
        image.putdata(pixels)
        path = self.path / name
        image.save(path, icc_profile=self.profile if profile is None else profile)
        return path

    def test_registered_crops_and_channel_statistics(self):
        reference = self.save("reference.png", [(0, 10, 20), (30, 40, 50)])
        candidate = self.save("candidate.png", [(2, 8, 20), (30, 44, 49)])
        result = compare_screen.compare(reference, candidate, (0, 0, 2, 1), (0, 0, 2, 1))
        self.assertEqual(result["different_pixels"], 2)
        self.assertEqual(result["mean_absolute_difference_rgb_8bit"], [1, 3, 0.5])
        self.assertEqual(result["max_absolute_difference_rgb_8bit"], [2, 4, 1])

    def test_rejects_different_profiles_out_of_bounds_and_transparency(self):
        reference = self.save("reference.png", [(0, 0, 0)] * 2)
        altered_profile = bytearray(self.profile)
        altered_profile[84] ^= 1  # ICC profile ID; color transform remains unchanged.
        different = self.save("different.png", [(0, 0, 0)] * 2, profile=bytes(altered_profile))
        transparent = self.save("transparent.png", [(0, 0, 0, 0)] * 2, mode="RGBA")
        with self.assertRaisesRegex(ValueError, "ICC profiles differ"):
            compare_screen.compare(reference, different, (0, 0, 2, 1), (0, 0, 2, 1))
        with self.assertRaisesRegex(ValueError, "beyond"):
            compare_screen.compare(reference, reference, (1, 0, 2, 1), (0, 0, 2, 1))
        with self.assertRaisesRegex(ValueError, "opaque"):
            compare_screen.compare(reference, transparent, (0, 0, 2, 1), (0, 0, 2, 1))

    def test_rejects_jpeg_and_missing_profile(self):
        reference = self.save("reference.png", [(0, 0, 0)] * 2)
        jpeg = self.path / "candidate.jpg"
        Image.new("RGB", (2, 1)).save(jpeg, icc_profile=self.profile)
        missing = self.path / "missing.png"
        Image.new("RGB", (2, 1)).save(missing)
        for candidate, message in [(jpeg, "lossless PNG"), (missing, "ICC profile")]:
            with self.assertRaisesRegex(ValueError, message):
                compare_screen.compare(reference, candidate, (0, 0, 2, 1), (0, 0, 2, 1))

    def test_rejects_invalid_and_non_rgb_profiles(self):
        reference = self.save("reference.png", [(0, 0, 0)] * 2)
        invalid = self.save("invalid.png", [(0, 0, 0)] * 2, profile=b"bad")
        lab_profile = ImageCms.ImageCmsProfile(ImageCms.createProfile("LAB")).tobytes()
        lab = self.save("lab.png", [(0, 0, 0)] * 2, profile=lab_profile)
        for candidate, message in [(invalid, "invalid ICC"), (lab, "must describe RGB")]:
            with self.assertRaisesRegex(ValueError, message):
                compare_screen.compare(reference, candidate, (0, 0, 2, 1), (0, 0, 2, 1))


if __name__ == "__main__":
    unittest.main()
