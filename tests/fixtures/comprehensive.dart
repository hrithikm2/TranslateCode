typedef Mapper<T, R> = R Function(T value);

enum Role { admin, member, guest }

mixin Timestamped {
  final DateTime createdAt = DateTime.utc(2026, 1, 1);

  String get stamp => createdAt.toIso8601String();
}

abstract interface class Greeter<T> {
  T greet(String prefix);
}

base class Entity {
  final int id;

  const Entity(this.id);
}

final class User extends Entity with Timestamped implements Greeter<String> {
  static const String domain = 'example.dev';

  String _name;
  Role role;
  final List<String> tags;
  int? _score;

  User(
    super.id,
    this._name, {
    this.role = Role.member,
    List<String>? tags,
  }) : tags = tags ?? <String>[];

  User.guest(int id) : this(id, 'Guest', role: Role.guest);

  factory User.fromMap(Map<String, Object?> map) => User(
        map['id']! as int,
        map['name']! as String,
        role: Role.values.byName(map['role']! as String),
      );

  String get name => _name;

  set name(String value) {
    if (value.trim().isEmpty) {
      throw ArgumentError.value(value, 'name');
    }
    _name = value.trim();
  }

  int get score => _score ?? 0;

  set score(int? value) => _score = value;

  @override
  String greet(String prefix) => '$prefix $_name@$domain';

  T map<T>(T Function(User user) convert) => convert(this);

  Future<int> loadScore() async {
    await Future<void>.delayed(Duration.zero);
    return _score ?? 0;
  }

  void addTag(String tag) {
    if (!tags.contains(tag)) tags.add(tag);
  }

  String roleLabel() {
    return switch (role) {
      Role.admin => 'administrator',
      Role.member => 'member',
      Role.guest => 'guest',
    };
  }

  @override
  bool operator ==(Object other) =>
      identical(this, other) || other is User && id == other.id;

  @override
  int get hashCode => id.hashCode;
}

sealed class Result<T> {
  const Result();
}

final class Success<T> extends Result<T> {
  final T value;
  const Success(this.value);
}

final class Failure<T> extends Result<T> {
  final String message;
  const Failure(this.message);
}

extension IntegerIterableX on Iterable<int> {
  int get sum => fold(0, (total, value) => total + value);
}

String describeResult(Result<int> result) => switch (result) {
      Success<int>(value: final value) => 'success:$value',
      Failure<int>(message: final message) => 'failure:$message',
    };

Future<void> main() async {
  final admin = User.fromMap(<String, Object?>{
    'id': 1,
    'name': 'Ada',
    'role': 'admin',
  })
    ..score = 42
    ..addTag('compiler');

  final guest = User.guest(2)
    ..score = null
    ..addTag('reader');

  final users = <User>[admin, guest];
  final names = users.map((user) => user.name).toList(growable: false);
  final ids = <int>[0, ...users.map((user) => user.id)];
  final Mapper<User, String> format =
      (user) => '${user.id}:${user.roleLabel()}:${user.score}';

  for (final user in users) {
    print(user.greet('hello'));
  }

  var index = 0;
  while (index < names.length) {
    print('name[$index]=${names[index]}');
    index++;
  }

  do {
    index--;
  } while (index > 0);

  switch (admin.role) {
    case Role.admin:
      print('privileged');
    case Role.member:
    case Role.guest:
      print('standard');
  }

  try {
    admin.name = '  Grace  ';
    assert(admin.name == 'Grace');
    print(format(admin));
    print('sum=${ids.sum}');
    print('score=${await admin.loadScore()}');
    print(describeResult(const Success<int>(7)));
  } on ArgumentError catch (error, stackTrace) {
    print('invalid:$error:$stackTrace');
  } finally {
    print('done:${admin.stamp.substring(0, 10)}');
  }
}
