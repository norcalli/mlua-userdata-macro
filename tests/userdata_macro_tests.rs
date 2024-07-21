use expect_test::expect;
use mlua_userdata_macro::generate_userdata;

#[generate_userdata]
mod my_module {
    #[derive(Debug, Clone)]
    pub struct MyStruct {
        pub value: i32,
        moo: i32,
    }

    impl MyStruct {
        pub fn new(value: i32) -> Self {
            Self { value, moo: 123 }
        }

        pub fn increment(&mut self) {
            self.value += 1;
        }

        pub fn get_v(&self) -> i32 {
            self.value * 2
        }

        pub fn get_moo(&self) -> i32 {
            self.moo
        }

        pub fn set_v(&mut self, value: i32) {
            self.value = value;
        }

        pub fn add_multiple(&mut self, a: i32, b: i32) {
            self.value += a + b;
        }
    }
}

#[cfg(test)]
mod tests {
    use self::my_module::MyStruct;
    use mlua::Lua;

    use super::*;

    #[test]
    fn test_userdata_methods() {
        let lua = Lua::new();

        let globals = lua.globals();
        let my_struct = my_module::MyStruct::new(10);
        globals.set("my_struct", my_struct).unwrap();

        lua.load(
            r#"
            my_struct:increment()
            local value = my_struct.value
            assert(value == 11)

            my_struct.value = 20
            value = my_struct.value
            assert(value == 20)

            my_struct:add_multiple(5, 7)
            value = my_struct.value
            assert(value == 32)

            assert(my_struct.moo == 123)

            assert(my_struct.v == 2 * my_struct.value)
            my_struct.v = 10
            assert(my_struct.value == 10)
            assert(my_struct.v == 2 * my_struct.value)
        "#,
        )
        .exec()
        .unwrap();

        let result: i32 = globals
            .get::<_, my_module::MyStruct>("my_struct")
            .unwrap()
            .value;
        expect!["10"].assert_eq(&result.to_string());
    }

    /* #[test]
    fn test_from_lua() {
        let lua = Lua::new();

        let my_struct: my_module::MyStruct = lua.load(r#"{ value = 42 }"#).eval().unwrap();

        expect![[r#"
            42
        "#]]
        .assert_eq(&my_struct.get_value().to_string());
    } */

    #[test]
    fn test_new_function() {
        let lua = Lua::new();

        lua.globals()
            .set("MyStruct", MyStruct::free_functions_table(&lua).unwrap())
            .unwrap();

        lua.load(
            r#"
            local instance = MyStruct.new(50)
            assert(instance.value == 50)
            assert(instance.v == 2 * instance.value)
        "#,
        )
        .exec()
        .unwrap();
    }
}
