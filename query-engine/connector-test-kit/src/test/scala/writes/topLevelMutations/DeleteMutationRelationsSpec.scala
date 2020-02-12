package writes.topLevelMutations

import org.scalatest.{FlatSpec, Matchers}
import util.ConnectorCapability.JoinRelationLinksCapability
import util._

class DeleteMutationRelationsSpec extends FlatSpec with Matchers with ApiSpecBase with SchemaBaseV11 {
  override def runOnlyForCapabilities: Set[ConnectorCapability] = Set(JoinRelationLinksCapability)

  "a P1! to C1! relation " should "error when deleting the parent" in {
    schemaWithRelation(onParent = ChildReq, onChild = ParentReq).test { t =>
      val project = SchemaDsl.fromStringV11() {
        t.datamodel
      }
      database.setup(project)

      server
        .query(
          """mutation {
          |  createChild(data: {
          |    c: "c1"
          |    parentReq: {
          |      create: {p: "p1"}
          |    }
          |  }){
          |    id
          |  }
          |}""",
          project
        )

      server.queryThatMustFail(
        s"""
         |mutation {
         |  deleteParent(
         |  where: {p: "p1"}
         |  ){
         |  p
         |  }
         |}
      """,
        project,
        errorCode = 0, // 3042,
        errorContains = """Error occurred during query execution:\nInterpretationError(\"Error for binding \\'3\\': RelationViolation(RelationViolation { relation_name: \\\"ChildToParent\\\", model_a_name: \\\"Child\\\", model_b_name: \\\"Parent\\\" })"""
          // "The change you are trying to make would violate the required relation 'ChildToParent' between Child and Parent"
      )

    }
  }

  "a P1! to C1! relation " should "error when deleting the parent2" in {
    schemaWithRelation(onParent = ChildReq, onChild = ParentReq).test { t =>
      val project = SchemaDsl.fromStringV11() {
        t.datamodel
      }
      database.setup(project)

      val res = server
        .query(
          """mutation {
          |  createParent(data: {
          |    p: "p1"
          |    childReq: {
          |      create: {c: "c1"}
          |    }
          |  }){
          |    id
          |    childReq{
          |       id
          |    }
          |  }
          |}""",
          project
        )
      val childId  = res.pathAsString("data.createParent.childReq.id")
      val parentId = res.pathAsString("data.createParent.id")

      server.queryThatMustFail(
        s"""
         |mutation {
         |  deleteParent(
         |  where: {id: "$parentId"}
         |  ){
         |  p
         |  }
         |}
      """,
        project,
        errorCode = 0, // 3042,
        errorContains = """Error occurred during query execution:\nInterpretationError(\"Error for binding \\'3\\': RelationViolation(RelationViolation { relation_name: \\\"ChildToParent\\\", model_a_name: \\\"Child\\\", model_b_name: \\\"Parent\\\" })\"""
      )

    }
  }

  "a P1! to C1 relation" should "succeed when trying to delete the parent" in {
    schemaWithRelation(onParent = ChildReq, onChild = ParentOpt).test { t =>
      val project = SchemaDsl.fromStringV11() {
        t.datamodel
      }
      database.setup(project)

      val res = server
        .query(
          """mutation {
          |  createParent(data: {
          |    p: "p1"
          |    childReq: {
          |      create: {c: "c1"}
          |    }
          |  }){
          |  id
          |    childReq{
          |       id
          |    }
          |  }
          |}""",
          project
        )

      val parentId = res.pathAsString("data.createParent.id")

      server.query(
        s"""
         |mutation {
         |  deleteParent(
         |  where: {id: "$parentId"}
         |  ){
         |  p
         |  }
         |}
      """,
        project
      )
    }
  }

  "a P1 to C1  relation " should "succeed when trying to delete the parent" in {
    schemaWithRelation(onParent = ChildOpt, onChild = ParentOpt).test { t =>
      val project = SchemaDsl.fromStringV11() {
        t.datamodel
      }
      database.setup(project)

      val res = server
        .query(
          """mutation {
          |  createParent(data: {
          |    p: "p1"
          |    childOpt: {
          |      create: {c: "c1"}
          |    }
          |  }){
          |    id
          |    childOpt{
          |       id
          |    }
          |  }
          |}""",
          project
        )

      val parentId = res.pathAsString("data.createParent.id")

      server.query(
        s"""
         |mutation {
         |  deleteParent(
         |  where: {id: "$parentId"}
         |  ){
         |  p
         |  }
         |}
      """,
        project
      )

    }
  }

  "a P1 to C1  relation " should "succeed when trying to delete the parent if there are no children" in {
    schemaWithRelation(onParent = ChildOpt, onChild = ParentOpt).test { t =>
      val project = SchemaDsl.fromStringV11() {
        t.datamodel
      }
      database.setup(project)

      server
        .query(
          """mutation {
          |  createParent(data: {
          |    p: "p1"
          |  }){
          |    id
          |  }
          |}""",
          project
        )

      server.query(
        s"""
         |mutation {
         |  deleteParent(
         |  where: {p: "p1"}
         |  ){
         |  p
         |  }
         |}
      """,
        project
      )

    }
  }

  "a PM to C1!  relation " should "error when deleting the parent" in {
    schemaWithRelation(onParent = ChildList, onChild = ParentReq).test { t =>
      val project = SchemaDsl.fromStringV11() {
        t.datamodel
      }
      database.setup(project)

      server.query(
        """mutation {
        |  createParent(data: {
        |    p: "p1"
        |    childrenOpt: {
        |      create: {c: "c1"}
        |    }
        |  }){
        |    childrenOpt{
        |       c
        |    }
        |  }
        |}""",
        project
      )

      server.queryThatMustFail(
        s"""
         |mutation {
         |  deleteParent(
         |  where: {p: "p1"}
         |  ){
         |  p
         |  }
         |}
      """,
        project,
        errorCode = 0, // 3042,
        errorContains = """Error occurred during query execution:\nInterpretationError(\"Error for binding \\'3\\': RelationViolation(RelationViolation { relation_name: \\\"ChildToParent\\\", model_a_name: \\\"Child\\\", model_b_name: \\\"Parent\\\" })""",
          // "The change you are trying to make would violate the required relation 'ChildToParent' between Child and Parent"
      )

    }
  }

  "a PM to C1!  relation " should "succeed if no child exists that requires the parent" in {
    schemaWithRelation(onParent = ChildList, onChild = ParentReq).test { t =>
      val project = SchemaDsl.fromStringV11() {
        t.datamodel
      }
      database.setup(project)

      server.query(
        """mutation {
        |  createParent(data: {
        |    p: "p1"
        |  }){
        |    childrenOpt{
        |       c
        |    }
        |  }
        |}""",
        project
      )

      server.query(
        s"""
         |mutation {
         |  deleteParent(
         |  where: {p: "p1"}
         |  ){
         |  p
         |  }
         |}
      """,
        project
      )

    }

  }

  "a P1 to C1!  relation " should "error when trying to delete the parent" in {
    schemaWithRelation(onParent = ChildOpt, onChild = ParentReq).test { t =>
      val project = SchemaDsl.fromStringV11() {
        t.datamodel
      }
      database.setup(project)

      server.query(
        """mutation {
        |  createParent(data: {
        |    p: "p1"
        |    childOpt: {
        |      create: {c: "c1"}
        |    }
        |  }){
        |    childOpt{
        |       c
        |    }
        |  }
        |}""",
        project
      )

      server.queryThatMustFail(
        s"""
         |mutation {
         |  deleteParent(
         |  where: {p: "p1"}
         |  ){
         |  p
         |  }
         |}
      """,
        project,
        errorCode = 0, // 3042,
        errorContains = """Error occurred during query execution:\nInterpretationError(\"Error for binding \\'3\\': RelationViolation(RelationViolation { relation_name: \\\"ChildToParent\\\", model_a_name: \\\"Child\\\", model_b_name: \\\"Parent\\\" })\")"""
      )
    }
  }

  "a P1 to C1!  relation " should "succeed when trying to delete the parent if there is no child" in {
    schemaWithRelation(onParent = ChildOpt, onChild = ParentReq).test { t =>
      val project = SchemaDsl.fromStringV11() {
        t.datamodel
      }
      database.setup(project)

      server.query(
        """mutation {
        |  createParent(data: {
        |    p: "p1"
        |
        |  }){
        |    p
        |  }
        |}""",
        project
      )

      server.query(
        s"""
         |mutation {
         |  deleteParent(
         |  where: {p: "p1"}
         |  ){
         |  p
         |  }
         |}
      """,
        project
      )
    }
  }

  "a PM to C1 " should "succeed in deleting the parent" in {
    schemaWithRelation(onParent = ChildList, onChild = ParentOpt).test { t =>
      val project = SchemaDsl.fromStringV11() {
        t.datamodel
      }
      database.setup(project)

      server
        .query(
          """mutation {
          |  createParent(data: {
          |    p: "p1"
          |    childrenOpt: {
          |      create: [{c: "c1"}, {c: "c2"}]
          |    }
          |  }){
          |    childrenOpt{
          |       c
          |    }
          |  }
          |}""",
          project
        )

      server.query(
        s"""
         |mutation {
         |  deleteParent(
         |  where: { p: "p1"}
         |  ){
         |    p
         |  }
         |}
      """,
        project
      )

    }
  }

  "a PM to C1 " should "succeed in deleting the parent if there is no child" in {
    schemaWithRelation(onParent = ChildList, onChild = ParentOpt).test { t =>
      val project = SchemaDsl.fromStringV11() {
        t.datamodel
      }
      database.setup(project)

      server
        .query(
          """mutation {
          |  createParent(data: {
          |    p: "p1"
          |  }){
          |    p
          |  }
          |}""",
          project
        )

      server.query(
        s"""
         |mutation {
         |  deleteParent(
         |  where: { p: "p1"}
         |  ){
         |    p
         |  }
         |}
      """,
        project
      )

    }
  }

  "a P1! to CM  relation" should "should succeed in deleting the parent " in {
    schemaWithRelation(onParent = ChildReq, onChild = ParentList).test { t =>
      val project = SchemaDsl.fromStringV11() {
        t.datamodel
      }
      database.setup(project)

      server.query(
        """mutation {
        |  createParent(data: {
        |    p: "p1"
        |    childReq: {
        |      create: {c: "c1"}
        |    }
        |  }){
        |    childReq{
        |       c
        |    }
        |  }
        |}""",
        project
      )

      server.query(
        s"""
         |mutation {
         |  deleteParent(
         |  where: {p: "p1"}
         |  ){
         |    p
         |  }
         |}
      """,
        project
      )

    }
  }

  "a P1 to CM  relation " should " should succeed in deleting the parent" in {
    schemaWithRelation(onParent = ChildOpt, onChild = ParentList).test { t =>
      val project = SchemaDsl.fromStringV11() {
        t.datamodel
      }
      database.setup(project)

      server.query(
        """mutation {
        |  createParent(data: {
        |    p: "p1"
        |    childOpt: {
        |      create: {c: "c1"}
        |    }
        |  }){
        |    childOpt{
        |       c
        |    }
        |  }
        |}""",
        project
      )

      server.query(
        s"""
         |mutation {
         |  deleteParent(
         |  where: {p: "p1"}
         |  ){
         |    p
         |  }
         |}
      """,
        project
      )

    }
  }

  "a P1 to CM  relation " should " should succeed in deleting the parent if there is no child" in {
    schemaWithRelation(onParent = ChildOpt, onChild = ParentList).test { t =>
      val project = SchemaDsl.fromStringV11() {
        t.datamodel
      }
      database.setup(project)

      server.query(
        """mutation {
        |  createParent(data: {
        |    p: "p1"
        |
        |  }){
        |    p
        |  }
        |}""",
        project
      )

      server.query(
        s"""
         |mutation {
         |  deleteParent(
         |  where: {p: "p1"}
         |  ){
         |    p
         |  }
         |}
      """,
        project
      )

    }
  }

  "a PM to CM  relation" should "succeed in deleting the parent" in {
    schemaWithRelation(onParent = ChildList, onChild = ParentList).test { t =>
      val project = SchemaDsl.fromStringV11() {
        t.datamodel
      }
      database.setup(project)

      server.query(
        """mutation {
        |  createParent(data: {
        |    p: "p1"
        |    childrenOpt: {
        |      create: [{c: "c1"},{c: "c2"}]
        |    }
        |  }){
        |    childrenOpt{
        |       c
        |    }
        |  }
        |}""",
        project
      )

      server.query(
        s"""
         |mutation {
         |  deleteParent(
         |  where: {p: "p1"}
         |  ){
         |    p
         |  }
         |}
      """,
        project
      )

    }
  }

  "a PM to CM  relation" should "succeed in deleting the parent if there is no child" in {
    schemaWithRelation(onParent = ChildList, onChild = ParentList).test { t =>
      val project = SchemaDsl.fromStringV11() {
        t.datamodel
      }
      database.setup(project)

      server.query(
        """mutation {
        |  createParent(data: {
        |    p: "p1"
        |
        |  }){
        |    p
        |  }
        |}""",
        project
      )

      server.query(
        s"""
         |mutation {
         |  deleteParent(
         |  where: {p: "p1"}
         |  ){
         |    p
         |  }
         |}
      """,
        project
      )

    }

  }

  "a PM to CM  relation" should "delete the parent from other relations as well" in {
    val testDataModels = {
      val dm1 = """model Parent {
                  | id           String     @id @default(cuid())
                  | p            String     @unique
                  | childrenOpt  Child[]    @relation(references: [id])
                  | stepChildOpt StepChild? @relation(references: [id])
                  |}
                  |
                  |model Child {
                  | id         String   @id @default(cuid())
                  | c          String   @unique
                  | parentsOpt Parent[]
                  |}
                  |
                  |model StepChild {
                  | id        String  @id @default(cuid())
                  | s         String  @unique
                  | parentOpt Parent?
                  |}
                """

      val dm2 = """model Parent {
                  | id           String  @id @default(cuid())
                  | p            String  @unique
                  | childrenOpt  Child[]
                  | stepChildOpt StepChild?
                  |}
                  |
                  |model Child {
                  | id         String   @id @default(cuid())
                  | c          String   @unique
                  | parentsOpt Parent[] @relation(references: [id])
                  |}
                  |
                  |model StepChild {
                  | id        String  @id @default(cuid())
                  | s         String  @unique
                  | parentOpt Parent? @relation(references: [id])
                  |}
                """

      val dm3 = """model Parent {
                  | id           String     @id @default(cuid())
                  | p            String     @unique
                  | childrenOpt  Child[]
                  | stepChildOpt StepChild?
                  |}
                  |
                  |model Child {
                  | id         String @id @default(cuid())
                  | c          String @unique
                  | parentsOpt Parent[]
                  |}
                  |
                  |model StepChild {
                  | id        String  @id @default(cuid())
                  | s         String  @unique
                  | parentOpt Parent? @relation(references: [id])
                  |}
                """

      val dm4 = """model Parent {
                  | id           String    @id @default(cuid())
                  | p            String     @unique
                  | childrenOpt  Child[]
                  | stepChildOpt StepChild? @relation(references: [id])
                  |}
                  |
                  |model Child {
                  | id         String   @id @default(cuid())
                  | c          String   @unique
                  | parentsOpt Parent[]
                  |}
                  |
                  |model StepChild {
                  | id        String  @id @default(cuid())
                  | s         String  @unique
                  | parentOpt Parent?
                  |}
                """

      TestDataModels(mongo = Vector(dm1, dm2), sql = Vector(dm3, dm4))
    }
    testDataModels.testV11 { project =>
      server.query(
        """mutation {
        |  createParent(data: {
        |    p: "p1"
        |    childrenOpt: {
        |      create: [{c: "c1"},{c: "c2"}]
        |    }
        |    stepChildOpt: {
        |      create: {s: "s1"}
        |    }
        |  }){
        |    p
        |  }
        |}""",
        project
      )

      server.query(
        s"""
         |mutation {
         |  deleteParent(
         |  where: { p: "p1"}
         | ){
         |  p
         |  }
         |}
      """,
        project
      )

    }
  }
}
