import { defineFunctionBuilders } from "../defineFunction.js";
import * as mml from "../buildMathML.js";

defineFunctionBuilders({
  type: "ordgroup",
  mathmlBuilder(group, style) {
    return mml.buildExpressionRow(group.body, style, group.semisimple);
  }
});
