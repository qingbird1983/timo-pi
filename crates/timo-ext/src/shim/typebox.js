// TypeBox 最小子集：输出纯 JSON Schema（设计 01 §1.1）。
const OptionalKind = Symbol.for("timo.typebox.optional");

function withOptions(base, options) {
  return Object.assign({}, base, options || {});
}

export const Type = {
  Object(properties, options) {
    const props = {};
    const required = [];
    for (const [key, value] of Object.entries(properties || {})) {
      if (value && value[OptionalKind]) {
        props[key] = value.schema;
      } else {
        props[key] = value;
        required.push(key);
      }
    }
    const out = { type: "object", properties: props };
    if (required.length > 0) {
      out.required = required;
    }
    return withOptions(out, options);
  },
  Optional(schema) {
    return { [OptionalKind]: true, schema };
  },
  String(options) {
    return withOptions({ type: "string" }, options);
  },
  Number(options) {
    return withOptions({ type: "number" }, options);
  },
  Integer(options) {
    return withOptions({ type: "integer" }, options);
  },
  Boolean(options) {
    return withOptions({ type: "boolean" }, options);
  },
  Null(options) {
    return withOptions({ type: "null" }, options);
  },
  Any(options) {
    return options || {};
  },
  Unknown(options) {
    return options || {};
  },
  Array(items, options) {
    return withOptions({ type: "array", items }, options);
  },
  Tuple(items, options) {
    return withOptions(
      { type: "array", items, minItems: items.length, maxItems: items.length },
      options,
    );
  },
  Literal(value, options) {
    return withOptions({ const: value }, options);
  },
  Union(schemas, options) {
    const literals = schemas.every((schema) =>
      Object.prototype.hasOwnProperty.call(schema, "const"),
    );
    const base = literals
      ? { enum: schemas.map((schema) => schema.const) }
      : { anyOf: schemas };
    return withOptions(base, options);
  },
  Record(key, value, options) {
    return withOptions(
      { type: "object", additionalProperties: value, propertyNames: key },
      options,
    );
  },
};

export default Type;
