"""Regenerate the original saved-path interoperability fixture with psd-tools 1.10.8."""
from pathlib import Path
from psd_tools import PSDImage
from psd_tools.constants import Tag
from psd_tools.psd import descriptor, vector
from psd_tools.psd.image_resources import ImageResource
from psd_tools.psd.layer_and_mask import GlobalLayerMaskInfo, LayerInfo
from psd_tools.psd.tagged_blocks import TaggedBlock, TaggedBlocks

def corner(y, x):
    return vector.ClosedKnotUnlinked((y,x),(y,x),(y,x))

def main():
    image = PSDImage.new("RGB", (8, 4), color=(20, 80, 190))
    curve = vector.Path([
        vector.PathFillRule(), vector.InitialFillRule(0),
        vector.ClosedPath([corner(0,0),corner(0,1),corner(1,1),corner(1,0)],operation=0),
        vector.OpenPath([
            vector.OpenKnotLinked((0,-0.25),(0.25,0),(0.5,0.25)),
            vector.OpenKnotUnlinked((0.5,0.75),(0.75,1),(1,1.25)),
        ]),
    ])
    image.image_resources[2000] = ImageResource(key=2000, name="Café", data=curve.tobytes())
    info = descriptor.Descriptor(name="\0", classID=b"pathInfoClass")
    info[b"pathUnicodeName"] = descriptor.String("輪廓 Café\0")
    names = descriptor.DescriptorBlock(name="\0", classID=b"pathsDataClass")
    names[b"pathList"] = descriptor.List([info])
    metadata = image._record.layer_and_mask_information
    metadata.layer_info = LayerInfo()
    metadata.global_layer_mask_info = GlobalLayerMaskInfo()
    metadata.tagged_blocks = TaggedBlocks()
    metadata.tagged_blocks[Tag.UNICODE_PATH_NAME] = TaggedBlock(key=Tag.UNICODE_PATH_NAME, data=names)
    with Path(__file__).with_name("saved-path.psd").open("wb") as output:
        image._record.write(output)

if __name__ == "__main__":
    main()
