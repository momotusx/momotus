/// Index-based list change from the core (`ListDiff` in mq-runtime).
enum ListDiff<T: Decodable & Sendable>: Decodable, Sendable {
    case append([T])
    case clear
    case pushFront(T)
    case pushBack(T)
    case popFront
    case popBack
    case insert(index: Int, value: T)
    case set(index: Int, value: T)
    case remove(index: Int)
    case truncate(length: Int)
    case reset([T])

    private enum Key: String, CodingKey { case op, values, value, index, length }

    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: Key.self)
        switch try c.decode(String.self, forKey: .op) {
        case "append": self = .append(try c.decode([T].self, forKey: .values))
        case "clear": self = .clear
        case "pushFront": self = .pushFront(try c.decode(T.self, forKey: .value))
        case "pushBack": self = .pushBack(try c.decode(T.self, forKey: .value))
        case "popFront": self = .popFront
        case "popBack": self = .popBack
        case "insert":
            self = .insert(index: try c.decode(Int.self, forKey: .index), value: try c.decode(T.self, forKey: .value))
        case "set":
            self = .set(index: try c.decode(Int.self, forKey: .index), value: try c.decode(T.self, forKey: .value))
        case "remove": self = .remove(index: try c.decode(Int.self, forKey: .index))
        case "truncate": self = .truncate(length: try c.decode(Int.self, forKey: .length))
        case "reset": self = .reset(try c.decode([T].self, forKey: .values))
        case let op:
            throw DecodingError.dataCorruptedError(forKey: .op, in: c, debugDescription: "unknown list op \(op)")
        }
    }
}

/// Applies core diffs in order and returns the new list. Mirrors
/// `ListDiff::apply` in mq-runtime and `applyDiffs` in the web app; where the
/// core would panic (an index past the end) this clamps or ignores instead,
/// like the web app does.
func applyDiffs<T>(_ list: [T], _ diffs: [ListDiff<T>]) -> [T] {
    var list = list
    for diff in diffs {
        switch diff {
        case .append(let values): list.append(contentsOf: values)
        case .clear: list.removeAll()
        case .pushFront(let value): list.insert(value, at: 0)
        case .pushBack(let value): list.append(value)
        case .popFront: if !list.isEmpty { list.removeFirst() }
        case .popBack: if !list.isEmpty { list.removeLast() }
        case .insert(let index, let value): list.insert(value, at: min(index, list.count))
        case .set(let index, let value): if list.indices.contains(index) { list[index] = value }
        case .remove(let index): if list.indices.contains(index) { list.remove(at: index) }
        case .truncate(let length): if length < list.count { list.removeLast(list.count - length) }
        case .reset(let values): list = values
        }
    }
    return list
}
