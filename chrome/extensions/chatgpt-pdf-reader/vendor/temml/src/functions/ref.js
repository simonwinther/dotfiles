import defineFunction from "../defineFunction.js";
import { AnchorNode } from "../domTree.js";
import { invalidIdRegEx } from "./label.js";

defineFunction({
  type: "ref",
  names: ["\\ref", "\\eqref"],
  props: {
    numArgs: 1,
    argTypes: ["raw"]
  },
  handler({ parser, funcName }, args) {
    return {
      type: "ref",
      mode: parser.mode,
      funcName,
      string: args[0].string.replace(invalidIdRegEx, "")
    };
  },
  mathmlBuilder(group, style) {
    // Create an empty <a> node. Set a class and an href attribute.
    // The post-processor will populate with the target's tag or equation number.
    const classes = group.funcName === "\\ref" ? ["tml-ref"] : ["tml-ref", "tml-eqref"]
    return new AnchorNode("#" + group.string, classes, null)
  }
});
